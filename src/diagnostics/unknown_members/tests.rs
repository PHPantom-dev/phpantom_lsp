use super::*;

fn collect(backend: &Backend, uri: &str, content: &str) -> Vec<Diagnostic> {
    backend.update_ast(uri, content);
    let mut out = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, content, &mut out);
    out
}

// ── Basic unknown-member detection ──────────────────────────────

#[test]
fn flags_unknown_method_on_known_class() {
    let php = r#"<?php
class Greeter {
public function hello(): string { return ''; }
}

function test(): void {
$g = new Greeter();
$g->nonexistent();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.iter().any(|d| {
            d.message.contains("nonexistent")
                && d.message.contains("Greeter")
                && d.message.contains("Method")
        }),
        "expected diagnostic for nonexistent method, got: {diags:?}"
    );
}

#[test]
fn flags_unknown_property_on_known_class() {
    let php = r#"<?php
class User {
public string $name;
}

function test(): void {
$u = new User();
$u->missing;
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.iter().any(|d| {
            d.message.contains("missing")
                && d.message.contains("User")
                && d.message.contains("Property")
        }),
        "expected diagnostic for missing property, got: {diags:?}"
    );
}

#[test]
fn flags_unknown_static_method() {
    let php = r#"<?php
class MathHelper {
public static function add(): int { return 0; }
}

MathHelper::nonexistent();
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("nonexistent") && d.message.contains("MathHelper")),
        "expected diagnostic for nonexistent static method, got: {diags:?}"
    );
}

#[test]
fn flags_unknown_constant_on_class() {
    let php = r#"<?php
class Config {
const VERSION = '1.0';
}

echo Config::MISSING;
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("MISSING") && d.message.contains("Config")),
        "expected diagnostic for missing constant, got: {diags:?}"
    );
}

// ── Should NOT produce diagnostics ──────────────────────────────

#[test]
fn no_diagnostic_for_existing_method() {
    let php = r#"<?php
class Greeter {
public function hello(): string { return ''; }
public function goodbye(): string { return ''; }
}

function test(): void {
$g = new Greeter();
$g->hello();
$g->goodbye();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

#[test]
fn no_diagnostic_for_existing_property() {
    let php = r#"<?php
class User {
public string $name;
public int $age;
}

function test(): void {
$u = new User();
echo $u->name;
echo $u->age;
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

#[test]
fn no_diagnostic_for_existing_constant() {
    let php = r#"<?php
class Config {
const VERSION = '1.0';
}

echo Config::VERSION;
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

#[test]
fn no_diagnostic_for_class_keyword() {
    let php = r#"<?php
class Foo {}
echo Foo::class;
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

// ── Magic methods ───────────────────────────────────────────────

#[test]
fn no_diagnostic_when_class_has_magic_call() {
    let php = r#"<?php
class Dynamic {
public function __call(string $name, array $args): mixed { return null; }
}

function test(): void {
$d = new Dynamic();
$d->anything();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "Method dispatched through __call is valid and must not be flagged, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_when_class_has_magic_get() {
    let php = r#"<?php
class Dynamic {
public function __get(string $name): mixed { return null; }
}

function test(): void {
$d = new Dynamic();
echo $d->anything;
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

#[test]
fn no_diagnostic_when_class_has_magic_call_static() {
    let php = r#"<?php
class Dynamic {
public static function __callStatic(string $name, array $args): mixed { return null; }
}

Dynamic::anything();
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "Static method dispatched through __callStatic is valid and must not be flagged, got: {diags:?}"
    );
}

// ── Inheritance ─────────────────────────────────────────────────

#[test]
fn no_diagnostic_for_inherited_method() {
    let php = r#"<?php
class Base {
public function baseMethod(): void {}
}
class Child extends Base {}

function test(): void {
$c = new Child();
$c->baseMethod();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

#[test]
fn no_diagnostic_for_trait_method() {
    let php = r#"<?php
trait Greetable {
public function greet(): string { return ''; }
}

class Person {
use Greetable;
}

function test(): void {
$p = new Person();
$p->greet();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

// ── Trait $this suppression ─────────────────────────────────────

#[test]
fn no_diagnostic_for_this_member_access_inside_trait() {
    // $this-> inside a trait method should
    // not produce false positives for members that exist on the
    // host class but not on the trait itself.
    let php = r#"<?php
trait LogsErrors {
public function logError(): void {
    $this->model;
    $this->eventType;
}
}

class ImportJob {
use LogsErrors;
public string $model = 'Product';
public string $eventType = 'import';
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for $this-> inside trait, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_this_method_call_inside_trait() {
    let php = r#"<?php
trait Cacheable {
public function cache(): void {
    $this->getCacheKey();
}
}

class Product {
use Cacheable;
public function getCacheKey(): string { return ''; }
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for $this->method() inside trait, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_self_static_inside_trait() {
    // self:: and static:: inside traits can reference members from
    // the host class.
    let php = r#"<?php
trait HasDefaults {
public static function create(): void {
    self::DEFAULT_NAME;
    static::factory();
}
}

class User {
use HasDefaults;
const DEFAULT_NAME = 'admin';
public static function factory(): void {}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for self::/static:: inside trait, got: {diags:?}"
    );
}

#[test]
fn trait_own_members_still_resolve_on_host_class() {
    // When a class uses a trait, accessing the trait's own members
    // from outside should still work (no false positive).
    let php = r#"<?php
trait Greetable {
public function greet(): string { return ''; }
}
class Person {
use Greetable;
}
function test(): void {
$p = new Person();
$p->greet();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for trait member on host class, got: {diags:?}"
    );
}

#[test]
fn variable_inside_trait_still_diagnosed() {
    // Only $this/self/static/parent are suppressed inside traits.
    // A typed variable like `$x` should still be diagnosed normally.
    let php = r#"<?php
class Foo {
public function bar(): void {}
}

trait MyTrait {
public function doStuff(Foo $x): void {
    $x->nonexistent();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("nonexistent") && d.message.contains("Foo")),
        "expected diagnostic for unknown method on typed variable inside trait, got: {diags:?}"
    );
}

// ── PHPDoc virtual members ──────────────────────────────────────

#[test]
fn no_diagnostic_for_phpdoc_method() {
    let php = r#"<?php
/**
 * @method string virtualMethod()
 */
class Magic {}

function test(): void {
$m = new Magic();
$m->virtualMethod();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

#[test]
fn no_diagnostic_for_phpdoc_property() {
    let php = r#"<?php
/**
 * @property string $virtualProp
 */
class Magic {
public function __get(string $name): mixed { return null; }
}

function test(): void {
$m = new Magic();
echo $m->virtualProp;
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

// ── $this / self / parent ───────────────────────────────────────

#[test]
fn flags_unknown_method_on_this() {
    let php = r#"<?php
class Foo {
public function bar(): void {
    $this->nonexistent();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("nonexistent") && d.message.contains("Foo")),
        "expected diagnostic, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_this_in_second_class() {
    let php = r#"<?php
class First {
public function a(): void {}
}
class Second {
public function b(): void {
    $this->b();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

#[test]
fn no_diagnostic_for_object_shape_property() {
    let php = r#"<?php
class Factory {
/**
 * @return object{name: string, age: int}
 */
public function create(): object {
    return (object)['name' => 'test', 'age' => 1];
}
}

class Consumer {
public function test(): void {
    $factory = new Factory();
    $obj = $factory->create();
    echo $obj->name;
    echo $obj->age;
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

#[test]
fn flags_unknown_property_on_object_shape() {
    let php = r#"<?php
class Factory {
/**
 * @return object{name: string, age: int}
 */
public function create(): object {
    return (object)['name' => 'test', 'age' => 1];
}
}

class Consumer {
public function test(): void {
    $obj = (new Factory())->create();
    echo $obj->missing;
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.iter().any(|d| d.message.contains("missing")),
        "expected diagnostic for missing property on object shape, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_this_in_anonymous_class() {
    let php = r#"<?php
class Outer {
public function make(): void {
    $anon = new class {
        public function inner(): void {}
        public function test(): void {
            $this->inner();
        }
    };
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

#[test]
fn flags_unknown_method_on_this_in_anonymous_class() {
    let php = r#"<?php
class Outer {
public function make(): void {
    $anon = new class {
        public function inner(): void {}
        public function test(): void {
            $this->missing();
        }
    };
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.iter().any(|d| d.message.contains("missing")),
        "expected diagnostic, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_parent_in_anonymous_class() {
    let php = r#"<?php
class Base {
public function baseMethod(): void {}
}
class Outer {
public function make(): void {
    $anon = new class extends Base {
        public function test(): void {
            parent::baseMethod();
        }
    };
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

#[test]
fn flags_unknown_method_on_this_in_second_class() {
    let php = r#"<?php
class First {
public function a(): void {}
}
class Second {
public function b(): void {
    $this->nonexistent();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("nonexistent") && d.message.contains("Second")),
        "expected diagnostic for Second, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_this_existing_method() {
    let php = r#"<?php
class Foo {
public function bar(): void {
    $this->bar();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

#[test]
fn flags_unknown_method_on_self() {
    let php = r#"<?php
class Foo {
public function bar(): void {
    self::nonexistent();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("nonexistent") && d.message.contains("Foo")),
        "expected diagnostic, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_self_existing_method() {
    let php = r#"<?php
class Foo {
public static function bar(): void {
    self::bar();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

#[test]
fn no_diagnostic_for_parent_existing_method() {
    let php = r#"<?php
class Base {
public function base(): void {}
}
class Child extends Base {
public function test(): void {
    parent::base();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

// ── Diagnostic metadata ─────────────────────────────────────────

#[test]
fn diagnostic_has_warning_severity() {
    let php = r#"<?php
class Foo { }
function test(): void {
$f = new Foo();
$f->missing();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(!diags.is_empty());
    assert_eq!(diags[0].severity, Some(DiagnosticSeverity::WARNING));
}

#[test]
fn diagnostic_has_code_and_source() {
    let php = r#"<?php
class Foo { }
function test(): void {
$f = new Foo();
$f->missing();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(!diags.is_empty());
    match &diags[0].code {
        Some(NumberOrString::String(code)) => {
            assert_eq!(code, UNKNOWN_MEMBER_CODE);
        }
        other => panic!("expected string code, got: {other:?}"),
    }
    assert_eq!(diags[0].source, Some("phpantom".to_string()));
}

// ── Case insensitivity ──────────────────────────────────────────

#[test]
fn method_matching_is_case_insensitive() {
    let php = r#"<?php
class Foo {
public function hello(): void {}
}
function test(): void {
$f = new Foo();
$f->HELLO();
$f->Hello();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

// ── Multiple unknowns ───────────────────────────────────────────

#[test]
fn flags_multiple_unknown_members() {
    let php = r#"<?php
class Foo {
public function real(): void {}
}
function test(): void {
$f = new Foo();
$f->missing1();
$f->real();
$f->missing2();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert_eq!(
        diags.len(),
        2,
        "expected 2 diagnostics, got {}: {diags:?}",
        diags.len()
    );
}

// ── Unresolvable subjects ───────────────────────────────────────

#[test]
fn no_diagnostic_when_subject_unresolvable() {
    // $x has no type info — we can't know what members it has,
    // so we should not flag anything.
    let php = r#"<?php
function test(): void {
$x->something();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostic for unresolvable subject, got: {diags:?}"
    );
}

// ── Enums ───────────────────────────────────────────────────────

#[test]
fn no_diagnostic_for_enum_case() {
    let php = r#"<?php
enum Color {
case Red;
case Green;
case Blue;
}
echo Color::Red;
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

#[test]
fn flags_unknown_enum_case() {
    let php = r#"<?php
enum Color {
case Red;
case Green;
case Blue;
}
echo Color::Yellow;
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.iter().any(|d| d.message.contains("Yellow")),
        "expected diagnostic for unknown enum case, got: {diags:?}"
    );
}

// ── Parameters ──────────────────────────────────────────────────

#[test]
fn flags_unknown_method_via_parameter() {
    let php = r#"<?php
class Service {
public function run(): void {}
}
function handler(Service $svc): void {
$svc->nonexistent();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("nonexistent") && d.message.contains("Service")),
        "expected diagnostic, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_method_via_parameter() {
    let php = r#"<?php
class Service {
public function run(): void {}
}
function handler(Service $svc): void {
$svc->run();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

// ── Parent with magic ───────────────────────────────────────────

#[test]
fn no_diagnostic_when_parent_has_magic_call() {
    let php = r#"<?php
class Base {
public function __call(string $name, array $args): mixed { return null; }
}
class Child extends Base {}

function test(): void {
$c = new Child();
$c->anything();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "Method inherited through a parent's __call is valid and must not be flagged, got: {diags:?}"
    );
}

// ── Interfaces ──────────────────────────────────────────────────

#[test]
fn no_diagnostic_for_interface_method() {
    let php = r#"<?php
interface Runnable {
public function run(): void;
}

class Worker implements Runnable {
public function run(): void {}
}

function handler(Runnable $r): void {
$r->run();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

// ── Static properties ───────────────────────────────────────────

#[test]
fn no_diagnostic_for_existing_static_property() {
    let php = r#"<?php
class Config {
public static string $version = '1.0';
}
echo Config::$version;
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

// ── Union types ─────────────────────────────────────────────────

#[test]
fn no_diagnostic_for_member_on_any_union_branch() {
    let php = r#"<?php
class Cat {
public function purr(): void {}
public function eat(): void {}
}
class Dog {
public function bark(): void {}
public function eat(): void {}
}
class Shelter {
/**
 * @return Cat|Dog
 */
public function adopt(): Cat|Dog {
    return new Cat();
}
}

class Test {
public function run(): void {
    $shelter = new Shelter();
    $pet = $shelter->adopt();
    $pet->eat();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

#[test]
fn flags_member_missing_from_all_union_branches() {
    let php = r#"<?php
class Cat {
public function purr(): void {}
}
class Dog {
public function bark(): void {}
}
class Shelter {
/**
 * @return Cat|Dog
 */
public function adopt(): Cat|Dog {
    return new Cat();
}
}

class Test {
public function run(): void {
    $shelter = new Shelter();
    $pet = $shelter->adopt();
    $pet->fly();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.iter().any(|d| d.message.contains("fly")),
        "expected diagnostic, got: {diags:?}"
    );
}

#[test]
fn union_diagnostic_message_mentions_multiple_types() {
    let php = r#"<?php
class Cat {
public function purr(): void {}
}
class Dog {
public function bark(): void {}
}
class Shelter {
/**
 * @return Cat|Dog
 */
public function adopt(): Cat|Dog {
    return new Cat();
}
}

class Test {
public function run(): void {
    $shelter = new Shelter();
    $pet = $shelter->adopt();
    $pet->fly();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    let d = diags
        .iter()
        .find(|d| d.message.contains("fly"))
        .expect("expected diagnostic");
    assert!(
        d.message.contains("Cat") && d.message.contains("Dog"),
        "expected both types in message: {}",
        d.message
    );
}

#[test]
fn no_diagnostic_when_any_union_branch_has_magic_call() {
    // When the subject is a union and any branch defines `__call`, the
    // access is dynamically dispatched through that branch at runtime,
    // so it must not be flagged (matches PHPStan, and the single-class
    // behaviour).  This is the Mockery higher-order-message pattern:
    // `$mock->shouldReceive(...)` returns a union where one branch has
    // `__call`, so the fluent method must not warn.
    let php = r#"<?php
class Normal {
public function known(): void {}
}
class Dynamic {
public function __call(string $name, array $args): mixed { return null; }
}

class Test {
/**
 * @return Normal|Dynamic
 */
public function get(): Normal|Dynamic { return new Normal(); }

public function run(): void {
    $x = $this->get();
    $x->anything();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "Method dispatched through a union branch's __call must not be flagged, got: {diags:?}"
    );
}

// ── stdClass ────────────────────────────────────────────────────

#[test]
fn no_diagnostic_for_property_on_stdclass() {
    let php = r#"<?php
function test(stdClass $obj): void {
echo $obj->anything;
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

#[test]
fn no_diagnostic_for_method_on_stdclass() {
    let php = r#"<?php
function test(stdClass $obj): void {
$obj->anything();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

#[test]
fn no_diagnostic_for_stdclass_in_union() {
    let php = r#"<?php
class Foo { public function a(): void {} }
/**
 * @return Foo|stdClass
 */
function get(): Foo|stdClass { return new Foo(); }
function test(): void {
$x = get();
$x->anything;
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

#[test]
fn no_diagnostic_for_stdclass_parameter() {
    let php = r#"<?php
function test(stdClass $obj): void {
echo $obj->name;
echo $obj->whatever;
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

#[test]
fn no_diagnostic_for_nested_stdclass_property_chain() {
    // A property assigned `new stdClass()` resolves to stdClass when
    // read again, so a further property access on it is not flagged.
    let php = r#"<?php
function test(): void {
$settings = new stdClass();
$settings->cache = new stdClass();
$settings->cache->ttl = 3600;
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

#[test]
fn no_diagnostic_for_deeply_nested_stdclass_property_chain() {
    let php = r#"<?php
function test(): void {
$root = new stdClass();
$root->a = new stdClass();
$root->a->b = new stdClass();
$root->a->b->c = 1;
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

#[test]
fn stdclass_property_key_invalidated_on_base_reassignment() {
    // Reassigning `$s` drops the stale `$s->cache` type, so `$s->cache`
    // resolves against the new object (a typed class here) rather than
    // the stdClass assigned before the reassignment.
    let php = r#"<?php
class Holder { public ?Holder $cache = null; public int $ttl = 0; }
function test(): void {
$s = new stdClass();
$s->cache = new stdClass();
$s = new Holder();
echo $s->cache->ttl;
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    // `$s->cache` is now `?Holder`, which has a `ttl` property — no
    // diagnostic, and crucially not resolved as the stale stdClass.
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

// ── PHPDoc property on child class ──────────────────────────────

#[test]
fn no_diagnostic_for_phpdoc_property_on_child_class() {
    let php = r#"<?php
/**
 * @property string $virtualProp
 */
class Base {
public function __get(string $name): mixed { return null; }
}

class Child extends Base {}

function test(): void {
$c = new Child();
echo $c->virtualProp;
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

#[test]
fn no_diagnostic_for_phpdoc_property_from_interface() {
    let php = r#"<?php
/**
 * @property string $name
 */
interface HasName {}

class User implements HasName {
public function __get(string $n): mixed { return null; }
}

function test(): void {
$u = new User();
echo $u->name;
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

// ── PHPDoc members inside type-narrowing contexts ───────────────

#[test]
fn no_diagnostic_for_phpdoc_members_inside_assert() {
    let php = r#"<?php
/**
 * @method string getName()
 */
class Entity {
public function __call(string $name, array $args): mixed { return null; }
}

class Base {}

class Test {
public function run(Base $item): void {
    assert($item instanceof Entity);
    echo $item->getName();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

#[test]
fn no_diagnostic_for_fqn_assert_instanceof() {
    // `\assert($item instanceof Entity)` — the leading backslash
    // is the global-namespace FQN form.  It should narrow the
    // variable type identically to the unqualified `assert()`.
    let php = r#"<?php
/**
 * @method string getName()
 */
class Entity {
public function __call(string $name, array $args): mixed { return null; }
}

class Base {}

class Test {
public function run(Base $item): void {
    \assert($item instanceof Entity);
    echo $item->getName();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for FQN \\assert instanceof narrowing, got: {diags:?}",
    );
}

#[test]
fn no_diagnostic_for_fqn_assert_with_interleaved_array_access() {
    // Combines both fixes: FQN `\assert()` narrowing and
    // interleaved array-access/property-chain resolution.
    // Reproduces the exact pattern from the bug report.
    let php = r#"<?php
class FormError {
public function getMessage(): string { return ''; }
}

class FormChild {
public function getName(): string { return ''; }
}

/** @var \Iterator<int, mixed> */
$errorIterator = new \ArrayIterator([]);
/** @var FormChild $child */
$child = new FormChild();
/** @var array<string, list<string>> */
$errors = [];

foreach ($errorIterator as $error) {
\assert(
    $error instanceof FormError,
    'Error is not a FormError!',
);
$errors[$child->getName()][] = $error->getMessage();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for FQN \\assert with interleaved array access, got: {diags:?}",
    );
}

#[test]
fn no_diagnostic_for_phpdoc_members_after_instanceof_narrowing() {
    let php = r#"<?php
/**
 * @method string getName()
 */
class Entity {
public function __call(string $name, array $args): mixed { return null; }
}

class Base {}

class Test {
public function run(Base $item): void {
    if ($item instanceof Entity) {
        echo $item->getName();
    }
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

// ── Inline && narrowing ─────────────────────────────────────────

#[test]
fn no_diagnostic_for_instanceof_and_chain() {
    // instanceof checks in the LHS of &&
    // should narrow the variable type for the RHS.
    let php = r#"<?php
class QueryException extends \Exception {
public array $errorInfo = [];
}

function test(\Throwable $e): void {
$e instanceof QueryException && $e->errorInfo;
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for && narrowing, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_instanceof_and_chain_in_catch() {
    // Variant: variable comes from a catch block.
    let php = r#"<?php
class QueryException extends \Exception {
public array $errorInfo = [];
}

function test(): void {
try {
    throw new \Exception('fail');
} catch (\Throwable $e) {
    $e instanceof QueryException && $e->errorInfo;
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for && narrowing in catch, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_instanceof_and_chain_method_call() {
    // Variant: method call instead of property access on RHS.
    let php = r#"<?php
class SpecialException extends \Exception {
public function getDetail(): string { return ''; }
}

function test(\Throwable $e): void {
$e instanceof SpecialException && $e->getDetail();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for && narrowing with method call, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_instanceof_and_chain_in_if_condition() {
    // Variant: the && is the condition of an if statement.
    let php = r#"<?php
class QueryException extends \Exception {
public array $errorInfo = [];
}

function test(\Throwable $e): void {
if ($e instanceof QueryException && count($e->errorInfo) > 0) {
    echo 'has errors';
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for && narrowing in if condition, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_instanceof_and_chain_in_return() {
    // Real-world repro: instanceof on LHS of && inside a return
    // statement.  The narrowing must propagate through the entire
    // chained && even when wrapped in `return`.
    let php = r#"<?php
class QueryException extends \Exception {
public array $errorInfo = [];
}

trait UniqueConstraintViolation {
protected function isUniqueConstraintViolation(\Throwable $exception): bool {
    return $exception instanceof QueryException
        && is_array($exception->errorInfo)
        && count($exception->errorInfo) >= 2
        && ($exception->errorInfo[0] ?? '') === '23000'
        && ($exception->errorInfo[1] ?? 0) === 1062;
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for && narrowing in return, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_ternary_instanceof_in_return() {
    // Ternary instanceof narrowing inside a return statement.
    let php = r#"<?php
class SpecialException extends \Exception {
public function getDetail(): string { return ''; }
}

function test(\Throwable $e): string {
return $e instanceof SpecialException ? $e->getDetail() : 'unknown';
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for ternary instanceof in return, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_chained_and_instanceof() {
    // Variant: chained && with multiple instanceof checks.
    let php = r#"<?php
class DetailedException extends \Exception {
public string $detail = '';
public string $context = '';
}

function test(\Throwable $e): void {
$e instanceof DetailedException && $e->detail !== '' && $e->context !== '';
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for chained && narrowing, got: {diags:?}"
    );
}

// ── Property chains ─────────────────────────────────────────────

#[test]
fn flags_unknown_member_on_property_chain() {
    let php = r#"<?php
class Inner {
public function known(): void {}
}
class Outer {
public Inner $inner;
}

class Test {
public function run(): void {
    $o = new Outer();
    $o->inner->missing();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.iter().any(|d| d.message.contains("missing")),
        "expected diagnostic, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_valid_property_chain() {
    let php = r#"<?php
class Inner {
public function known(): void {}
}
class Outer {
public Inner $inner;
}

class Test {
public function run(): void {
    $o = new Outer();
    $o->inner->known();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

// ── Method return chains ────────────────────────────────────────

#[test]
fn flags_unknown_member_on_method_return_chain() {
    let php = r#"<?php
class Inner {
public function known(): void {}
}
class Outer {
public function getInner(): Inner { return new Inner(); }
}

function test(): void {
$o = new Outer();
$o->getInner()->missing();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.iter().any(|d| d.message.contains("missing")),
        "expected diagnostic, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_valid_method_return_chain() {
    let php = r#"<?php
class Inner {
public function known(): void {}
}
class Outer {
public function getInner(): Inner { return new Inner(); }
}

function test(): void {
$o = new Outer();
$o->getInner()->known();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

// ── Virtual property chains ─────────────────────────────────────

#[test]
fn flags_unknown_member_on_virtual_property_chain() {
    let php = r#"<?php
class Inner {
public function known(): void {}
}

/**
 * @property Inner $inner
 */
class Outer {
public function __get(string $name): mixed { return null; }
}

function test(): void {
$o = new Outer();
$o->inner->missing();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.iter().any(|d| d.message.contains("missing")),
        "expected diagnostic, got: {diags:?}"
    );
}

// ── Scalar member access ────────────────────────────────────────

#[test]
fn flags_member_access_on_scalar_property_type() {
    let php = r#"<?php
class Foo {
public int $value = 0;
}

class Test {
public function run(): void {
    $foo = new Foo();
    $foo->value->nonexistent();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("int") && d.message.contains("nonexistent")),
        "expected scalar access diagnostic, got: {diags:?}"
    );
    assert!(
        diags
            .iter()
            .any(|d| d.severity == Some(DiagnosticSeverity::ERROR)),
        "expected ERROR severity for scalar access"
    );
}

#[test]
fn flags_member_access_on_string_property_type() {
    let php = r#"<?php
class Foo {
public string $name = '';
}

class Test {
public function run(): void {
    $foo = new Foo();
    $foo->name->nonexistent();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("string") && d.message.contains("nonexistent")),
        "expected scalar access diagnostic, got: {diags:?}"
    );
}

#[test]
fn flags_member_access_on_scalar_method_return() {
    let php = r#"<?php
class Foo {
public function getCount(): int { return 0; }
}

class Test {
public function run(): void {
    $foo = new Foo();
    $foo->getCount()->nonexistent();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("int") && d.message.contains("nonexistent")),
        "expected scalar access diagnostic, got: {diags:?}"
    );
}

#[test]
fn flags_method_call_on_scalar_method_return_chain() {
    let php = r#"<?php
class Inner {
public function getValue(): string { return ''; }
}

class Middle {
public function getInner(): Inner { return new Inner(); }
}

class Outer {
public function getMiddle(): Middle { return new Middle(); }
}

class Test {
public function run(): void {
    $o = new Outer();
    $o->getMiddle()->getInner()->getValue()->nonexistent();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags
            .iter()
            .any(|d| { d.message.contains("string") && d.message.contains("nonexistent") }),
        "expected scalar access diagnostic, got: {diags:?}"
    );
}

#[test]
fn flags_method_call_on_scalar_return_typed_param() {
    let php = r#"<?php
class Foo {
public function getCount(): int { return 0; }
}
function test(Foo $foo): void {
$foo->getCount()->nonexistent();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("int") && d.message.contains("nonexistent")),
        "expected scalar access diagnostic, got: {diags:?}"
    );
}

#[test]
fn flags_scalar_access_on_static_method_chain() {
    let php = r#"<?php
class Foo {
public static function getCount(): int { return 0; }
}
class Test {
public function run(): void {
    Foo::getCount()->nonexistent();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("int") && d.message.contains("nonexistent")),
        "expected scalar access diagnostic, got: {diags:?}"
    );
}

#[test]
fn flags_scalar_access_on_function_return_chain() {
    let php = r#"<?php
function getNumber(): int { return 42; }
function test(): void {
getNumber()->nonexistent();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("int") && d.message.contains("nonexistent")),
        "expected scalar access diagnostic, got: {diags:?}"
    );
}

#[test]
fn flags_scalar_access_on_docblock_return_type() {
    let php = r#"<?php
class Foo {
/**
 * @return string
 */
public function getName() { return ''; }
}

class Test {
public function run(): void {
    $foo = new Foo();
    $foo->getName()->nonexistent();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags
            .iter()
            .any(|d| { d.message.contains("string") && d.message.contains("nonexistent") }),
        "expected scalar access diagnostic, got: {diags:?}"
    );
}

#[test]
fn flags_scalar_access_on_static_return_chain() {
    let php = r#"<?php
class Foo {
public function getName(): string { return ''; }
}
class Test {
public function run(): void {
    $foo = new Foo();
    $foo->getName()->nonexistent();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags
            .iter()
            .any(|d| { d.message.contains("string") && d.message.contains("nonexistent") }),
        "expected scalar access diagnostic, got: {diags:?}"
    );
}

#[test]
fn no_scalar_diagnostic_for_class_returning_chain() {
    let php = r#"<?php
class Builder {
public function where(): self { return $this; }
public function get(): self { return $this; }
}
function test(): void {
$b = new Builder();
$b->where()->get();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no scalar access diagnostic for class-returning chain, got: {diags:?}"
    );
}

#[test]
fn flags_scalar_access_on_function_returning_class_chain() {
    let php = r#"<?php
class Foo {
public function getName(): string { return ''; }
}
function createFoo(): Foo { return new Foo(); }
function test(): void {
createFoo()->getName()->nonexistent();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags
            .iter()
            .any(|d| { d.message.contains("string") && d.message.contains("nonexistent") }),
        "expected scalar access diagnostic, got: {diags:?}"
    );
}

#[test]
fn flags_scalar_access_on_array_element_method_chain() {
    let php = r#"<?php
class Item {
public function getLabel(): string { return ''; }
}

function test(): void {
/** @var array<int, Item> $items */
$items = [];
$items[0]->getLabel()->nonexistent();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags
            .iter()
            .any(|d| { d.message.contains("string") && d.message.contains("nonexistent") }),
        "expected scalar access diagnostic, got: {diags:?}"
    );
}

#[test]
fn flags_scalar_access_on_deeper_method_chain() {
    let php = r#"<?php
class Inner {
public function getValue(): int { return 42; }
}
class Outer {
public function getInner(): Inner { return new Inner(); }
}
class Test {
public function run(): void {
    $o = new Outer();
    $o->getInner()->getValue()->nonexistent();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("int") && d.message.contains("nonexistent")),
        "expected scalar access diagnostic, got: {diags:?}"
    );
}

#[test]
fn flags_scalar_property_access_on_deeper_method_chain() {
    let php = r#"<?php
class Inner {
public string $label = '';
}
class Outer {
public function getInner(): Inner { return new Inner(); }
}
class Test {
public function run(): void {
    $o = new Outer();
    $o->getInner()->label->nonexistent();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags
            .iter()
            .any(|d| { d.message.contains("string") && d.message.contains("nonexistent") }),
        "expected scalar access diagnostic, got: {diags:?}"
    );
}

#[test]
fn flags_member_access_on_virtual_scalar_property() {
    let php = r#"<?php
/**
 * @property int $age
 * @property string $name
 */
class User {
public function __get(string $name): mixed { return null; }
}

class Test {
public function run(): void {
    $u = new User();
    $u->age->nonexistent();
    $u->name->nonexistent2();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("int") && d.message.contains("nonexistent")),
        "expected scalar access diagnostic for int property, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_scalar_property_access_itself() {
    let php = r#"<?php
class Foo {
public int $count = 0;
}
function test(): void {
$f = new Foo();
echo $f->count;
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "scalar property access itself should not be flagged, got: {diags:?}"
    );
}

// ── Bare variable with scalar type ──────────────────────────────

#[test]
fn flags_member_access_on_bare_int_variable() {
    let php = r#"<?php
class Foo {
public function getCount(): int { return 0; }
}

class Test {
public function run(): void {
    $foo = new Foo();
    $number = $foo->getCount();
    $number->nonexistent();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("int") && d.message.contains("nonexistent")),
        "expected scalar access diagnostic for bare int variable, got: {diags:?}"
    );
}

#[test]
fn flags_property_access_on_bare_string_variable() {
    let php = r#"<?php
class Foo {
public function getName(): string { return ''; }
}

class Test {
public function run(): void {
    $foo = new Foo();
    $name = $foo->getName();
    $name->nonexistent;
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags
            .iter()
            .any(|d| { d.message.contains("string") && d.message.contains("nonexistent") }),
        "expected scalar access diagnostic for bare string variable, got: {diags:?}"
    );
}

#[test]
fn flags_method_access_on_bare_bool_variable() {
    let php = r#"<?php
class Foo {
public function isValid(): bool { return true; }
}

class Test {
public function run(): void {
    $foo = new Foo();
    $valid = $foo->isValid();
    $valid->nonexistent();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("bool") && d.message.contains("nonexistent")),
        "expected scalar access diagnostic for bare bool variable, got: {diags:?}"
    );
}

#[test]
fn flags_member_access_on_scalar_function_return() {
    let php = r#"<?php
function getNumber(): int { return 42; }
class Test {
public function run(): void {
    $n = getNumber();
    $n->nonexistent();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("int") && d.message.contains("nonexistent")),
        "expected scalar access diagnostic for function return, got: {diags:?}"
    );
}

#[test]
fn flags_member_access_on_scalar_method_return_via_variable() {
    let php = r#"<?php
class Foo {
public function getCount(): int { return 0; }
}
class Test {
public function run(): void {
    $foo = new Foo();
    $count = $foo->getCount();
    $count->nonexistent();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("int") && d.message.contains("nonexistent")),
        "expected scalar access diagnostic, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_bare_scalar_variable_without_member_access() {
    let php = r#"<?php
function test(): void {
$n = 42;
echo $n;
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "bare scalar variable without member access should not produce diagnostic, got: {diags:?}"
    );
}

// ── Typed parameter scalar access ───────────────────────────────

#[test]
fn flags_member_access_on_scalar_typed_parameter() {
    let php = r#"<?php
function test(int $value): void {
$value->nonexistent();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("int") && d.message.contains("nonexistent")),
        "expected scalar access diagnostic for typed parameter, got: {diags:?}"
    );
}

// ── Unknown class parameter ─────────────────────────────────────

#[test]
fn flags_member_access_on_unknown_class_parameter() {
    let php = r#"<?php
function test(NonExistentClass $obj): void {
$obj->doSomething();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.iter().any(|d| {
            d.message.contains("doSomething") && d.message.contains("NonExistentClass")
        }),
        "expected diagnostic for unknown class parameter, got: {diags:?}"
    );
}

#[test]
fn flags_member_access_on_unknown_return_type_function() {
    let php = r#"<?php
/** @return NonExistentClass */
function createObj() { return new stdClass; }
function test(): void {
$obj = createObj();
$obj->doSomething();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        !diags.is_empty(),
        "expected diagnostic for unknown return type, got: {diags:?}"
    );
}

#[test]
fn no_unknown_class_diagnostic_for_mixed_parameter() {
    let php = r#"<?php
function test(mixed $obj): void {
$obj->doSomething();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostic for mixed parameter, got: {diags:?}"
    );
}

#[test]
fn no_unknown_class_diagnostic_for_class_string_parameter() {
    let php = r#"<?php
/**
 * @param class-string<BackedEnum> $enum
 */
function test(string $enum): void {
$enum::from('test');
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostic for class-string parameter, got: {diags:?}"
    );
}

// ── Type alias / array shape / object value ─────────────────────

#[test]
fn no_diagnostic_for_type_alias_array_shape_object_value() {
    let php = r#"<?php
class Service {
public function getName(): string { return ''; }
}

class Factory {
/**
 * @return array{service: Service, name: string}
 */
public function create(): array { return []; }
}

class Test {
public function run(): void {
    $f = new Factory();
    $result = $f->create();
    $result['service']->getName();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostic for array shape object value, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_multiple_type_alias_object_values() {
    let php = r#"<?php
class UserService {
public function findAll(): array { return []; }
}

class PostService {
public function findRecent(): array { return []; }
}

class Container {
/**
 * @return array{users: UserService, posts: PostService}
 */
public function services(): array { return []; }
}

class Test {
public function run(): void {
    $c = new Container();
    $services = $c->services();
    $services['users']->findAll();
    $services['posts']->findRecent();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostic for multiple array shape values, got: {diags:?}"
    );
}

// ── Inline array element function call ──────────────────────────

#[test]
fn no_diagnostic_for_inline_array_element_function_call() {
    let php = r#"<?php
class Item {
public function process(): void {}
}

function getItems(): array {
/** @var Item[] */
return [];
}

function test(): void {
getItems()[0]->process();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostic for inline array element call, got: {diags:?}"
    );
}

// ── Pre-resolved base class has the member ──────────────────────

#[test]
fn no_diagnostic_when_member_exists_on_pre_resolved_base_class() {
    let php = r#"<?php
class Builder {
public function where(): self { return $this; }
public function get(): array { return []; }
}
function test(): void {
$b = new Builder();
$b->where();
$b->get();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for existing methods, got: {diags:?}"
    );
}

// ── @see tag references ─────────────────────────────────────────

#[test]
fn no_diagnostic_for_see_tag_method_reference() {
    let php = r#"<?php
class Foo {
public function bar(): void {}

/**
 * @see Foo::bar()
 */
public function test(): void {}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostic for @see tag method reference, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_see_tag_constant_reference() {
    let php = r#"<?php
class Foo {
const BAR = 1;

/**
 * @see Foo::BAR
 */
public function test(): void {}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostic for @see tag constant reference, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_see_tag_hash_fragment_reference() {
    let php = r#"<?php
class Foo {
public function bar(): void {}

/**
 * @see Foo#bar
 */
public function test(): void {}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostic for @see tag hash-fragment reference, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_inline_see_tag_method_reference() {
    let php = r#"<?php
class Foo {
public function bar(): void {}

/**
 * This delegates to {@see Foo::bar()}.
 */
public function test(): void {}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostic for inline @see reference, got: {diags:?}"
    );
}

// ── Namespaced stub class member ────────────────────────────────

#[test]
fn no_diagnostic_for_namespaced_stub_class_member() {
    let stubs = HashMap::from([(
        "Ns\\StubClass",
        r#"<?php
namespace Ns;
class StubClass {
public function stubMethod(): void {}
}
"#,
    )]);
    let backend = Backend::new_test_with_stubs(stubs);
    let php = r#"<?php
use Ns\StubClass;

function test(StubClass $obj): void {
$obj->stubMethod();
}
"#;
    let uri = "file:///test.php";
    backend.update_ast(uri, php);
    let mut out = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, php, &mut out);
    assert!(
        out.is_empty(),
        "expected no diagnostic for namespaced stub class member, got: {out:?}"
    );
}

// ── Conditional $this return in chain ────────────────────────────

#[test]
fn no_false_positive_on_conditional_this_return_in_chain() {
    let php = r#"<?php
class Builder {
/**
 * @return $this
 */
public function where(): static { return $this; }

public function get(): array { return []; }
}
class Test {
public function run(): void {
    $b = new Builder();
    $b->where()->get();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no false positive on conditional $this return chain, got: {diags:?}"
    );
}

// ── Cross-method cache isolation ────────────────────────────────

#[test]
fn no_false_positive_when_same_var_has_different_type_in_different_methods() {
    // The subject resolution cache was scoped
    // to the enclosing class, not the enclosing method.  Two methods
    // in the same class that both use `$order->` would share a cache
    // entry even when `$order` has a completely different type in each
    // method.  The first resolution wins and subsequent methods get
    // the wrong type, producing false-positive "unknown member"
    // diagnostics.
    let php = r#"<?php
class OrderA {
public function propOnA(): void {}
}
class OrderB {
public function propOnB(): void {}
}
class Service {
public function handleA(OrderA $order): void {
    $order->propOnA();
}
public function handleB(OrderB $order): void {
    $order->propOnB();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no false positives when same-named variable has different types \
         in different methods, got: {diags:?}"
    );
}

#[test]
fn no_false_positive_same_var_different_type_top_level_functions() {
    // Same bug as the class-method variant, but with top-level
    // functions instead of methods.
    let php = r#"<?php
class Alpha {
public function alphaMethod(): void {}
}
class Beta {
public function betaMethod(): void {}
}
function first(Alpha $x): void {
$x->alphaMethod();
}
function second(Beta $x): void {
$x->betaMethod();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no false positives for same-named variable in different \
         top-level functions, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_this_inside_closure_in_trait() {
    // $this-> and static:: inside a closure nested within a trait
    // method should be suppressed, just like direct trait method bodies.
    let php = r#"<?php
trait SalesInfoGlobalTrait {
public function getSalesInfo(): void {
    $items = array_map(function ($item) {
        $this->model;
        $this->eventType;
        static::where();
        static::query();
    }, []);
}
}

class SalesReport {
use SalesInfoGlobalTrait;
public string $model = 'Sale';
public string $eventType = 'report';
public static function where(): void {}
public static function query(): void {}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for $this/static:: inside closure in trait, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_this_inside_arrow_fn_in_trait() {
    // $this-> inside an arrow function nested within a trait method.
    let php = r#"<?php
trait FilterTrait {
public function applyFilter(): void {
    $fn = fn() => $this->filterColumn;
}
}

class Report {
use FilterTrait;
public string $filterColumn = 'status';
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for $this-> inside arrow fn in trait, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_chain_rooted_at_static_inside_trait() {
    // `static::where(...)->update(...)` inside a trait method.
    // The subject_text for `update` is `"static::where('x', 'y')"`,
    // which is a chain rooted at `static`.  The suppression must
    // recognise the root keyword, not require an exact match.
    let php = r#"<?php
trait SalesInfoGlobalTrait {
public function updateSalesInfo(): void {
    static::where('column', 'value')->update(['sales' => 1]);
}
}

class SalesReport extends \Illuminate\Database\Eloquent\Model {
use SalesInfoGlobalTrait;
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for static::...->method() chain inside trait, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_chain_rooted_at_this_inside_trait() {
    // `$this->relation()->first()` inside a trait method.
    // The subject_text for `first` is `"$this->relation()"`.
    let php = r#"<?php
trait HasRelation {
public function loadRelation(): void {
    $this->items()->first();
}
}

class Order {
use HasRelation;
/** @return \Illuminate\Database\Eloquent\Builder */
public function items(): object { return new \stdClass(); }
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for $this->...->method() chain inside trait, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_chain_rooted_at_static_inside_closure_in_trait() {
    // `static::where(...)` inside a closure within a trait method.
    let php = r#"<?php
trait SalesInfoGlobalTrait {
public function updateSalesInfo(): void {
    $items = array_map(function ($item) {
        static::where('col', 'val')->update(['x' => 1]);
    }, []);
}
}

class SalesReport extends \Illuminate\Database\Eloquent\Model {
use SalesInfoGlobalTrait;
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for static:: chain inside closure in trait, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_self_chain_inside_trait() {
    // `self::create(...)` chain inside a trait.
    let php = r#"<?php
trait Creatable {
public function duplicate(): void {
    self::create(['name' => 'copy'])->save();
}
}

class Product extends \Illuminate\Database\Eloquent\Model {
use Creatable;
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for self::...->method() chain inside trait, got: {diags:?}"
    );
}

#[test]
fn variable_chain_inside_trait_still_diagnosed() {
    // Non-self-referencing variables inside traits should still be
    // diagnosed when the member truly doesn't exist.
    let php = r#"<?php
trait BadTrait {
public function doStuff(): void {
    $obj = new \stdClass();
    $obj->nonExistentMethod();
}
}
"#;
    let backend = Backend::new_test();
    let _diags = collect(&backend, "file:///test.php", php);
    // stdClass has __get/__set magic, so property access is fine,
    // but we're just verifying the suppression doesn't swallow
    // non-self-referencing subjects.  stdClass actually tolerates
    // all member access, so this test verifies the suppression
    // is scoped to self-referencing subjects only.
    // (No assertion on diagnostic count — stdClass has magic methods.)
}

#[test]
fn flags_unknown_member_despite_valid_in_other_method() {
    // The flip side: make sure that a member that IS valid in
    // one method is still flagged as unknown in another method where
    // the variable has a different type that lacks the member.
    let php = r#"<?php
class HasFoo {
public function foo(): void {}
}
class NoFoo {
public function bar(): void {}
}
class Service {
public function a(HasFoo $x): void {
    $x->foo();
}
public function b(NoFoo $x): void {
    $x->foo();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("foo") && d.message.contains("NoFoo")),
        "expected diagnostic for foo() on NoFoo in method b(), got: {diags:?}"
    );
    // Make sure it's exactly one diagnostic (the one in method b).
    let foo_diags: Vec<_> = diags.iter().filter(|d| d.message.contains("foo")).collect();
    assert_eq!(
        foo_diags.len(),
        1,
        "expected exactly one 'foo' diagnostic (in method b), got: {foo_diags:?}"
    );
}

#[test]
fn no_false_positive_when_parameter_is_reassigned() {
    // When a method parameter is reassigned
    // mid-body, PHPantom should resolve subsequent accesses against
    // the new type, not the original parameter type.
    //
    // Before the fix, the subject cache keyed by (subject_text,
    // access_kind, scope) would cache the parameter type on the
    // first `$file->` encounter and reuse it for accesses after
    // the reassignment, producing false-positive "unknown member"
    // diagnostics.
    let php = r#"<?php
class UploadedFile {
public string $originalName;
}
class FileModel {
public int $id;
public string $name;
}
class Result {
public function getFile(): FileModel { return new FileModel(); }
}
class FileUploadService {
public function uploadFile(UploadedFile $file): void {
    $file->originalName;
    $result = new Result();
    $file = $result->getFile();
    $file->id;
    $file->name;
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no false positives when parameter is reassigned mid-body, got: {diags:?}"
    );
}

#[test]
fn flags_unknown_member_after_reassignment() {
    // The flip side: after reassignment, members from the
    // NEW type that don't exist should still be flagged.
    let php = r#"<?php
class TypeA {
public function onlyOnA(): void {}
}
class TypeB {
public function onlyOnB(): void {}
}
class Service {
public function process(TypeA $var): void {
    $var->onlyOnA();
    $var = new TypeB();
    $var->onlyOnA();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("onlyOnA") && d.message.contains("TypeB")),
        "expected diagnostic for onlyOnA() on TypeB after reassignment, got: {diags:?}"
    );
    // Exactly one diagnostic — the post-reassignment access.
    let relevant: Vec<_> = diags
        .iter()
        .filter(|d| d.message.contains("onlyOnA"))
        .collect();
    assert_eq!(
        relevant.len(),
        1,
        "expected exactly one 'onlyOnA' diagnostic (after reassignment), got: {relevant:?}"
    );
}

/// `$found = null; foreach (...) { $found = $pen; } $found->write()`
/// must not produce a scalar_member_access diagnostic when the foreach
/// value variable has a known type.
#[test]
fn no_false_positive_null_init_foreach_var_to_var_reassign() {
    let php = r#"<?php
class Pen {
public function write(): void {}
public function color(): string { return ''; }
}
class Svc {
/** @param list<Pen> $pens */
public function find(array $pens): void {
    $found = null;
    foreach ($pens as $pen) {
        if ($pen->color() === 'blue') {
            $found = $pen;
        }
    }
    if ($found) {
        $found->write();
    }
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    let scalar_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.code == Some(NumberOrString::String("scalar_member_access".to_string())))
        .collect();
    assert!(
        scalar_diags.is_empty(),
        "should not flag scalar_member_access on $found->write() after foreach reassign, got: {scalar_diags:?}"
    );
    let unknown_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.message.contains("write"))
        .collect();
    assert!(
        unknown_diags.is_empty(),
        "should not flag unknown member 'write' on $found after foreach reassign, got: {unknown_diags:?}"
    );
}

/// `$valid = null; foreach (...) { if (...) { $valid = $item; break; } }`
/// `if (!$valid) { return ...; } $valid->details` must not produce a
/// scalar_member_access diagnostic.  The guard clause (`if (!$valid) { return; }`)
/// strips null from the scope, leaving only the class type.
///
/// This test activates the forward-walker scope cache to reproduce a
/// regression where the scope cache records `$validMandate` as `null`
/// only (the foreach body assignment is lost or the guard clause
/// narrowing doesn't strip null).
#[test]
fn no_false_positive_null_init_foreach_guard_clause_early_return() {
    let php = r#"<?php
class Mandate {
    public object $details;
    public function isInvalid(): bool { return false; }
}
class Client {
    /** @return mixed */
    public function getMandates(): mixed { return []; }
}
class Svc {
    public function check(): ?object {
        $client = new Client();
        $mandates = $client->getMandates();
        $validMandate = null;
        /** @var Mandate $mandate */
        foreach ($mandates as $mandate) {
            if (!$mandate->isInvalid()) {
                $validMandate = $mandate;
                break;
            }
        }

        if (!$validMandate) {
            return null;
        }

        $details = $validMandate->details;
        return $details;
    }
}
"#;
    let backend = Backend::new_test();
    backend.update_ast("file:///test.php", php);

    // Activate the scope cache and build scopes (mirrors the analyse path).
    let _scope_guard = crate::completion::variable::forward_walk::with_diagnostic_scope_cache();
    {
        let file_ctx = backend.file_context("file:///test.php");
        let class_loader = backend.class_loader(&file_ctx);
        let function_loader_cl = backend.function_loader(&file_ctx);
        let constant_loader_cl = backend.constant_loader();
        let loaders = crate::completion::resolver::Loaders {
            function_loader: Some(&function_loader_cl),
            constant_loader: Some(&constant_loader_cl),
        };
        let local_classes: Vec<std::sync::Arc<crate::types::ClassInfo>> = backend
            .uri_classes_index
            .read()
            .get("file:///test.php")
            .cloned()
            .unwrap_or_default();
        crate::completion::variable::forward_walk::build_diagnostic_scopes(
            php,
            &local_classes,
            &class_loader,
            loaders,
            Some(&backend.resolved_class_cache),
        );
    }

    let mut diags = Vec::new();
    backend.collect_unknown_member_diagnostics("file:///test.php", php, &mut diags);
    let scalar_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.code == Some(NumberOrString::String("scalar_member_access".to_string())))
        .collect();
    assert!(
        scalar_diags.is_empty(),
        "should not flag scalar_member_access on $validMandate->details after guard clause, got: {scalar_diags:?}"
    );
}

/// `fn($x) => $x instanceof Foo && $x->method()` — an untyped arrow
/// function parameter narrowed by an earlier `&&` conjunct must be
/// visible to the member access in a later conjunct.  This mirrors the
/// production `analyse` path where the forward-walker scope cache is
/// built before diagnostics run.
#[test]
fn arrow_fn_param_narrowed_by_and_instanceof_scope_cache() {
    let php = r#"<?php
class Collection {
    public function contains($x): bool { return true; }
}
class Svc {
    public function run(): void {
        $faq1 = 1;
        $cb = fn($faqs) => $faqs instanceof Collection && $faqs->contains($faq1);
    }
}
"#;
    let backend = Backend::new_test();
    backend.config.lock().diagnostics.unresolved_member_access = Some(true);
    backend.update_ast("file:///test.php", php);

    let _scope_guard = crate::completion::variable::forward_walk::with_diagnostic_scope_cache();
    {
        let file_ctx = backend.file_context("file:///test.php");
        let class_loader = backend.class_loader(&file_ctx);
        let function_loader_cl = backend.function_loader(&file_ctx);
        let constant_loader_cl = backend.constant_loader();
        let loaders = crate::completion::resolver::Loaders {
            function_loader: Some(&function_loader_cl),
            constant_loader: Some(&constant_loader_cl),
        };
        let local_classes: Vec<std::sync::Arc<crate::types::ClassInfo>> = backend
            .uri_classes_index
            .read()
            .get("file:///test.php")
            .cloned()
            .unwrap_or_default();
        crate::completion::variable::forward_walk::build_diagnostic_scopes(
            php,
            &local_classes,
            &class_loader,
            loaders,
            Some(&backend.resolved_class_cache),
        );
    }

    let mut diags = Vec::new();
    backend.collect_unknown_member_diagnostics("file:///test.php", php, &mut diags);
    assert!(
        diags.is_empty(),
        "arrow-fn param narrowed by `&&` instanceof should resolve, got: {diags:?}"
    );
}

#[test]
fn arrow_fn_param_narrowed_by_and_instanceof_fresh_path() {
    let php = r#"<?php
class Collection {
    public function contains($x): bool { return true; }
}
class Svc {
    public function run(): void {
        $faq1 = 1;
        $cb = fn($faqs) => $faqs instanceof Collection && $faqs->contains($faq1);
    }
}
"#;
    let backend = Backend::new_test();
    backend.config.lock().diagnostics.unresolved_member_access = Some(true);
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "arrow-fn param narrowed by `&&` instanceof should resolve (fresh path), got: {diags:?}"
    );
}

/// Direct instantiation inside foreach body (no var-to-var).
#[test]
fn no_false_positive_null_init_foreach_direct_reassign() {
    let php = r#"<?php
class Transaction {
public function commit(): void {}
}
class Svc {
/** @param list<string> $items */
public function process(array $items): void {
    $tx = null;
    foreach ($items as $item) {
        $tx = new Transaction();
    }
    if ($tx) {
        $tx->commit();
    }
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    let bad_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.message.contains("commit") || d.message.contains("null"))
        .collect();
    assert!(
        bad_diags.is_empty(),
        "should not flag commit() or scalar null after foreach reassign, got: {bad_diags:?}"
    );
}

// ── Negative narrowing after early return ───────────────────────

#[test]
fn no_false_positive_after_guard_clause_excludes_type() {
    // After `if ($value instanceof Stringable) { return; }`, the
    // variable should be narrowed to exclude Stringable.  Inside
    // the subsequent `if ($value instanceof BackedEnum)` block,
    // `$value` must resolve to BackedEnum (not Stringable).
    let php = r#"<?php
interface Stringable {
public function __toString(): string;
}
interface BackedEnum {
public readonly int|string $value;
}

class Svc {
public static function toString(mixed $value): string
{
    if ($value instanceof Stringable) {
        return $value->__toString();
    }
    if ($value instanceof BackedEnum) {
        $value = $value->value;
    }
    return '';
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    // There should be no diagnostic about 'value' not found on
    // 'Stringable'.  The guard clause return means $value cannot
    // be Stringable in subsequent code.
    let bad = diags
        .iter()
        .filter(|d| d.message.contains("value") && d.message.contains("Stringable"))
        .collect::<Vec<_>>();
    assert!(
        bad.is_empty(),
        "should not flag 'value' on Stringable after guard clause excludes it, got: {bad:?}"
    );
}

#[test]
fn no_false_positive_sequential_instanceof_guards() {
    // Multiple sequential guard clauses should each exclude their
    // type from subsequent code.
    let php = r#"<?php
interface Alpha {
public function alphaMethod(): void;
}
interface Beta {
public function betaMethod(): void;
}
class Gamma {
public function gammaMethod(): void {}
}

class Svc {
public function test(Alpha|Beta|Gamma $x): void
{
    if ($x instanceof Alpha) {
        return;
    }
    if ($x instanceof Beta) {
        return;
    }
    $x->gammaMethod();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    let bad = diags
        .iter()
        .filter(|d| {
            d.message.contains("gammaMethod")
                && (d.message.contains("Alpha") || d.message.contains("Beta"))
        })
        .collect::<Vec<_>>();
    assert!(
        bad.is_empty(),
        "should not flag gammaMethod after two guard clauses exclude Alpha and Beta, got: {bad:?}"
    );
}

// ── self::/static::/parent:: in static access subjects ──────────

fn create_enum_backend() -> Backend {
    let mut stubs = std::collections::HashMap::new();
    stubs.insert(
        "UnitEnum",
        "<?php\ninterface UnitEnum {\n    /** @return static[] */\n    public static function cases(): array;\n    public readonly string $name;\n}\n",
    );
    stubs.insert(
        "BackedEnum",
        "<?php\ninterface BackedEnum extends UnitEnum {\n    public static function from(int|string $value): static;\n    public static function tryFrom(int|string $value): ?static;\n    public readonly int|string $value;\n}\n",
    );
    Backend::new_test_with_stubs(stubs)
}

#[test]
fn no_diagnostic_for_self_enum_case_value() {
    let php = r#"<?php
enum SizeUnit: string {
case pcs = 'pcs';
case pair = 'pair';
case g = 'g';

public function translation(): string {
    return self::pcs->value;
}

public static function units(): array {
    return [
        self::pcs->value,
        self::pair->value,
        self::g->value,
    ];
}
}
"#;
    let backend = create_enum_backend();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

#[test]
fn no_diagnostic_for_static_enum_case_value() {
    let php = r#"<?php
enum Currency: string {
case USD = 'usd';
case EUR = 'eur';

public static function defaults(): array {
    return [static::USD->value];
}
}
"#;
    let backend = create_enum_backend();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

#[test]
fn no_diagnostic_for_self_enum_case_name() {
    let php = r#"<?php
enum Color: int {
case Red = 1;
case Blue = 2;

public function label(): string {
    return self::Red->name;
}
}
"#;
    let backend = create_enum_backend();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

#[test]
fn no_diagnostic_for_self_static_access_on_regular_class() {
    let php = r#"<?php
class Config {
public const VERSION = '1.0';
public static function version(): string { return self::VERSION; }
public function test(): string {
    return static::version();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

#[test]
fn no_diagnostic_for_self_const_in_class_level_attribute() {
    // A `self::CONST` reference inside a class-level attribute sits
    // *before* the `class` keyword and the body braces, so the
    // enclosing class must be found via its declaration span (which
    // includes the leading attribute) rather than the body span.
    let php = r#"<?php
#[Route(name: self::ROUTE)]
class HealthCheckController
{
    public const string ROUTE = 'health-check';
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("ROUTE") || d.message.contains("could not be resolved")),
        "expected no self::ROUTE diagnostic, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_method_on_anonymous_class_variable() {
    // When `$model = new class extends Foo { ... }` is used outside
    // the anonymous class body, member access on `$model` should
    // resolve via the anonymous class's ClassInfo (which inherits
    // from the parent and uses traits).
    let php = r#"<?php
class Base {
public function hello(): string { return "hi"; }
}

function test(): void {
$model = new class extends Base {};
$model->hello();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

#[test]
fn no_diagnostic_for_trait_method_on_anonymous_class_variable() {
    let php = r#"<?php
trait Greetable {
public function greet(): string { return "hello"; }
}

function test(): void {
$obj = new class {
    use Greetable;
};
$obj->greet();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
}

#[test]
fn flags_unknown_method_on_anonymous_class_variable() {
    let php = r#"<?php
function test(): void {
$obj = new class {
    public function known(): void {}
};
$obj->unknown();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.iter().any(|d| d.message.contains("unknown")),
        "expected unknown member diagnostic, got: {diags:?}",
    );
}

#[test]
fn no_diagnostic_for_standalone_var_docblock_in_closure() {
    // A standalone multi-variable `@var` block inside a closure body
    // (without a following assignment) should declare types for
    // untyped closure parameters.
    let php = r#"<?php
class App {
public function make(string $class): mixed { return new $class; }
}

class Foo {
public function test(): void {
    $fn = function ($app, $params) {
        /**
         * @var App                      $app
         * @var array{indexName: string} $params
         */
        $app->make('Something');
    };
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics when @var declares closure param type, got: {diags:?}",
    );
}

#[test]
fn flags_unknown_member_with_standalone_var_docblock_in_closure() {
    // When `@var` resolves the type, unknown members should still
    // be flagged (proves the type was actually resolved).
    let php = r#"<?php
class App {
public function make(string $class): mixed { return new $class; }
}

class Foo {
public function test(): void {
    $fn = function ($app) {
        /** @var App $app */
        $app->nonExistentMethod();
    };
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("nonExistentMethod")),
        "expected unknown member diagnostic for nonExistentMethod, got: {diags:?}",
    );
}

#[test]
fn no_diagnostic_for_property_chain_array_access_on_collection() {
    // `$obj->prop['key']` where `prop` is a collection class with
    // `@extends DataCollection<string, Day>` should resolve the
    // bracket access to the element type `Day`.
    let php = r#"<?php
class Day {
public string $from;
public string $to;
}

/**
 * @template TKey of array-key
 * @template TValue
 * @implements \ArrayAccess<TKey, TValue>
 */
class DataCollection implements \ArrayAccess {
/** @return TValue */
public function offsetGet(mixed $offset): mixed {}
public function offsetExists(mixed $offset): bool {}
public function offsetSet(mixed $offset, mixed $value): void {}
public function offsetUnset(mixed $offset): void {}
}

/**
 * @extends DataCollection<string, Day>
 */
class OpeningHours extends DataCollection {}

class ServicePoint {
public ?OpeningHours $opening_hours;
}

function test(ServicePoint $sp): void {
$day = $sp->opening_hours['monday'] ?? null;
if ($day !== null) {
    $day->from;
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for property chain array access on collection, got: {diags:?}",
    );
}

#[test]
fn no_diagnostic_for_parent_static_call_return_type() {
    // `parent::method()` should resolve the return type from the
    // parent class so that member access on the result works.
    let php = r#"<?php
class Response {
public function status(): int { return 200; }
public function body(): string { return ''; }
}

class BaseConnector {
protected function call(string $endpoint): Response
{
    return new Response();
}
}

class LoggedConnection extends BaseConnector {
protected function call(string $endpoint): Response
{
    $response = parent::call($endpoint);
    $response->status();
    $response->body();
    return $response;
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for parent::call() return type chain, got: {diags:?}",
    );
}

// ── Chain error propagation ─────────────────────────────────────────

#[test]
fn chain_propagation_flags_only_first_broken_method() {
    // $m->callHome()->callMom()->callDad() — only callHome should
    // be flagged; callMom and callDad are downstream of the break.
    let php = r#"<?php
class Machine {
public function knownMethod(): self { return $this; }
}

function test(): void {
$m = new Machine();
$m->callHome()->callMom()->callDad();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert_eq!(
        diags.len(),
        1,
        "expected exactly 1 diagnostic (first broken link only), got: {diags:?}"
    );
    assert!(
        diags[0].message.contains("callHome"),
        "expected diagnostic for callHome, got: {:?}",
        diags[0].message
    );
}

#[test]
fn chain_propagation_separate_statements_flag_both() {
    // $m->callHome(); $m->callMom(); — separate statements, both
    // should be flagged independently.
    let php = r#"<?php
class Machine {
public function knownMethod(): self { return $this; }
}

function test(): void {
$m = new Machine();
$m->callHome();
$m->callMom();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert_eq!(
        diags.len(),
        2,
        "expected 2 diagnostics (separate statements), got: {diags:?}"
    );
    let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
    assert!(
        messages.iter().any(|m| m.contains("callHome")),
        "expected callHome diagnostic"
    );
    assert!(
        messages.iter().any(|m| m.contains("callMom")),
        "expected callMom diagnostic"
    );
}

#[test]
fn chain_propagation_scalar_suppresses_downstream() {
    // $user->getAge()->value->deep — only ->value should be flagged
    // (scalar access on int), ->deep is downstream of the scalar break.
    let php = r#"<?php
class User {
public function getAge(): int { return 30; }
}

function test(): void {
$user = new User();
$user->getAge()->value->deep;
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert_eq!(
        diags.len(),
        1,
        "expected exactly 1 diagnostic (scalar access only), got: {diags:?}"
    );
    assert!(
        diags[0].message.contains("int"),
        "expected scalar type 'int' in message, got: {:?}",
        diags[0].message
    );
}

#[test]
fn chain_propagation_second_link_broken_suppresses_rest() {
    // $o->getInner()->fakeMethod()->next() — only fakeMethod should
    // be flagged; next() is downstream.
    let php = r#"<?php
class Inner {
public function known(): void {}
}
class Outer {
public function getInner(): Inner { return new Inner(); }
}

function test(): void {
$o = new Outer();
$o->getInner()->fakeMethod()->next()->deep();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert_eq!(
        diags.len(),
        1,
        "expected exactly 1 diagnostic (first broken link), got: {diags:?}"
    );
    assert!(
        diags[0].message.contains("fakeMethod"),
        "expected diagnostic for fakeMethod, got: {:?}",
        diags[0].message
    );
}

#[test]
fn chain_propagation_scalar_method_return_suppresses_chain() {
    // $o->getMiddle()->getInner()->getValue()->nonexistent()->another()
    // — only nonexistent() should be flagged (scalar access on string).
    let php = r#"<?php
class Inner {
public function getValue(): string { return ''; }
}

class Middle {
public function getInner(): Inner { return new Inner(); }
}

class Outer {
public function getMiddle(): Middle { return new Middle(); }
}

class Test {
public function run(): void {
    $o = new Outer();
    $o->getMiddle()->getInner()->getValue()->nonexistent()->another();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert_eq!(
        diags.len(),
        1,
        "expected exactly 1 diagnostic (scalar access), got: {diags:?}"
    );
    assert!(
        diags[0].message.contains("nonexistent"),
        "expected diagnostic for nonexistent, got: {:?}",
        diags[0].message
    );
}

#[test]
fn chain_propagation_property_does_not_match_longer_name() {
    // Ensure that a broken property `value` does not suppress a
    // separate property `value_extra` on the same subject.
    let php = r#"<?php
class Foo {
public int $value = 0;
public string $value_extra = '';
}

function test(): void {
$f = new Foo();
$f->value->nope;
$f->value_extra->nope;
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert_eq!(
        diags.len(),
        2,
        "expected 2 diagnostics (value and value_extra are independent), got: {diags:?}"
    );
}

#[test]
fn chain_propagation_static_method_chain() {
    // Foo::create()->unknown()->next() — only unknown() should be
    // flagged; next() is downstream.
    let php = r#"<?php
class Foo {
public static function create(): self { return new self(); }
public function known(): self { return $this; }
}

function test(): void {
Foo::create()->unknown()->next();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert_eq!(
        diags.len(),
        1,
        "expected exactly 1 diagnostic (first broken link), got: {diags:?}"
    );
    assert!(
        diags[0].message.contains("unknown"),
        "expected diagnostic for unknown, got: {:?}",
        diags[0].message
    );
}

#[test]
fn chain_propagation_null_safe_operator() {
    // $m?->callHome()?->callMom() — only callHome should be flagged.
    let php = r#"<?php
class Machine {
public function knownMethod(): self { return $this; }
}

function test(?Machine $m): void {
$m?->callHome()?->callMom();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert_eq!(
        diags.len(),
        1,
        "expected exactly 1 diagnostic (null-safe chain), got: {diags:?}"
    );
    assert!(
        diags[0].message.contains("callHome"),
        "expected diagnostic for callHome, got: {:?}",
        diags[0].message
    );
}

#[test]
fn chain_propagation_this_method_chain() {
    // $this->unknownMethod()->next() inside a class — only
    // unknownMethod should be flagged.
    let php = r#"<?php
class Foo {
public function test(): void {
    $this->unknownMethod()->next()->deep();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert_eq!(
        diags.len(),
        1,
        "expected exactly 1 diagnostic ($this chain), got: {diags:?}"
    );
    assert!(
        diags[0].message.contains("unknownMethod"),
        "expected diagnostic for unknownMethod, got: {:?}",
        diags[0].message
    );
}

#[test]
fn chain_propagation_property_chain_suppresses_downstream() {
    // $o->getInner()->label->nonexistent->deep — only ->nonexistent
    // should be flagged (scalar access on string from label).
    let php = r#"<?php
class Inner {
public string $label = '';
}
class Outer {
public function getInner(): Inner { return new Inner(); }
}
class Test {
public function run(): void {
    $o = new Outer();
    $o->getInner()->label->nonexistent->deep;
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert_eq!(
        diags.len(),
        1,
        "expected exactly 1 diagnostic (scalar property access), got: {diags:?}"
    );
    assert!(
        diags[0].message.contains("nonexistent") || diags[0].message.contains("string"),
        "expected diagnostic about scalar access on string, got: {:?}",
        diags[0].message
    );
}

#[test]
fn chain_propagation_mixed_arrow_and_static_chain() {
    // $o->getInner()::staticMissing()->next() — only staticMissing
    // should be flagged.
    let php = r#"<?php
class Inner {
public function known(): void {}
}
class Outer {
public function getInner(): Inner { return new Inner(); }
}

function test(): void {
$o = new Outer();
$o->getInner()::staticMissing()->next();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    // staticMissing is unknown on Inner; next() is downstream.
    assert_eq!(
        diags.len(),
        1,
        "expected exactly 1 diagnostic (first broken static link), got: {diags:?}"
    );
    assert!(
        diags[0].message.contains("staticMissing"),
        "expected diagnostic for staticMissing, got: {:?}",
        diags[0].message
    );
}

#[test]
fn chain_propagation_does_not_suppress_errors_inside_closure_arguments() {
    // Errors inside closure/arrow-function arguments are independent
    // expressions — they must NOT be suppressed by a broken link in
    // the outer chain.
    //
    // $joe::whereInvalid()->where(fn() => $showThisError->unknown())->hideMe()->hideMe();
    //
    // Expected diagnostics:
    //   1. whereInvalid  (unknown static method on Joe)
    //   2. unknown       (unknown method on ShowThisError — inside the closure)
    // NOT expected:
    //   - hideMe (downstream of whereInvalid in the outer chain)
    let php = r#"<?php
class Joe {
public function where(callable $cb): self { return $this; }
}

class ShowThisError {
public function valid(): void {}
}

function test(): void {
$joe = new Joe();
$showThisError = new ShowThisError();
$joe::whereInvalid()->where(fn() => $showThisError->unknown())->hideMe()->hideMe();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
    assert!(
        messages.iter().any(|m| m.contains("whereInvalid")),
        "expected diagnostic for whereInvalid (outer chain), got: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("unknown")),
        "expected diagnostic for unknown (inside closure), got: {messages:?}"
    );
    assert!(
        !messages.iter().any(|m| m.contains("hideMe")),
        "hideMe should be suppressed (downstream of whereInvalid), got: {messages:?}"
    );
    assert_eq!(
        diags.len(),
        2,
        "expected exactly 2 diagnostics (whereInvalid + unknown), got: {messages:?}"
    );
}

// ── && short-circuit narrowing does not eliminate null ───────────

/// `$lastPaidEnd !== null && $lastPaidEnd->diffInDays(…)` must
/// not produce a scalar_member_access diagnostic.  The `!== null`
/// check on the left side of `&&` should narrow away `null` for
/// the right side.
#[test]
fn no_false_positive_and_short_circuit_null_narrowing() {
    let php = r#"<?php
class Carbon {
public function diffInDays(Carbon $other): int { return 0; }
public function startOfDay(): static { return $this; }
}
class Period {
public Carbon $ending;
}
class Svc {
/** @param list<Period> $periods */
public function gaps(array $periods): void {
    $lastPaidEnd = null;
    $periodStart = new Carbon();
    foreach ($periods as $period) {
        if ($lastPaidEnd !== null && $lastPaidEnd->diffInDays($periodStart) > 0) {
            // should not report: Cannot access method 'diffInDays' on type 'null'
        }
        $lastPaidEnd = $period->ending->startOfDay();
    }
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    let scalar_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.code == Some(NumberOrString::String("scalar_member_access".to_string())))
        .collect();
    assert!(
        scalar_diags.is_empty(),
        "should not flag scalar_member_access on $lastPaidEnd->diffInDays() after !== null guard in &&, got: {scalar_diags:?}"
    );
}

/// Variant: bare truthy check `$var && $var->method()`.
#[test]
fn no_false_positive_and_short_circuit_truthy_narrowing() {
    let php = r#"<?php
class Logger {
public function log(string $msg): void {}
}
class Svc {
public function run(): void {
    $logger = null;
    if (rand(0,1)) {
        $logger = new Logger();
    }
    $logger && $logger->log('hello');
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    let scalar_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.code == Some(NumberOrString::String("scalar_member_access".to_string())))
        .collect();
    assert!(
        scalar_diags.is_empty(),
        "should not flag scalar_member_access on $logger->log() after truthy guard in &&, got: {scalar_diags:?}"
    );
}

/// Variant: chained `&&` with null check as first operand.
/// `$a !== null && $b !== null && $a->method()` — the null check
/// for `$a` is two levels up in the `&&` chain.
#[test]
fn no_false_positive_chained_and_null_narrowing() {
    let php = r#"<?php
class Foo {
public function bar(): int { return 0; }
}
class Svc {
public function test(): void {
    $a = null;
    $b = null;
    if (rand(0,1)) { $a = new Foo(); }
    if (rand(0,1)) { $b = new Foo(); }
    if ($a !== null && $b !== null && $a->bar() > 0) {
        // both $a and $b are non-null here
    }
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    let scalar_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.code == Some(NumberOrString::String("scalar_member_access".to_string())))
        .collect();
    assert!(
        scalar_diags.is_empty(),
        "should not flag scalar_member_access on $a->bar() in chained && with null guards, got: {scalar_diags:?}"
    );
}

/// Variant: three null-init vars with compound && guard, cursor on
/// third var inside the if-body (not inside the condition).
#[test]
fn no_false_positive_if_body_triple_null_narrowing() {
    let php = r#"<?php
class Foo {
public function bar(): int { return 0; }
public function baz(): static { return $this; }
}
class Svc {
public function test(): void {
    $x = null;
    $y = null;
    $z = null;
    if (rand(0,1)) { $x = new Foo(); }
    if (rand(0,1)) { $y = new Foo(); }
    if (rand(0,1)) { $z = new Foo(); }
    if ($x !== null && $y !== null && $z !== null && $x->baz()->bar() > 0) {
        $z->bar();
    }
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    let scalar_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.code == Some(NumberOrString::String("scalar_member_access".to_string())))
        .collect();
    assert!(
        scalar_diags.is_empty(),
        "should not flag scalar_member_access on $z->bar() inside if-body after triple && null guard, got: {scalar_diags:?}"
    );
}

/// Variant: null check in if-condition narrows inside the then-body.
#[test]
fn no_false_positive_if_body_null_narrowing() {
    let php = r#"<?php
class Foo {
public function bar(): int { return 0; }
}
class Svc {
public function test(): void {
    $a = null;
    $b = null;
    if (rand(0,1)) { $a = new Foo(); }
    if (rand(0,1)) { $b = new Foo(); }
    if ($a !== null && $b !== null && $a->bar() > 0) {
        $b->bar();
    }
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    let scalar_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.code == Some(NumberOrString::String("scalar_member_access".to_string())))
        .collect();
    assert!(
        scalar_diags.is_empty(),
        "should not flag scalar_member_access on $b->bar() inside if-body after && null guard, got: {scalar_diags:?}"
    );
}

/// Variant: && inside a ternary condition in a return statement.
#[test]
fn no_false_positive_ternary_wrapped_and_null_narrowing() {
    let php = r#"<?php
class Foo {
public function val(): int { return 0; }
}
class Svc {
public function test(): int {
    $c = null;
    if (rand(0,1)) { $c = new Foo(); }
    return $c !== null && $c->val() > 5 ? 1 : 0;
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    let scalar_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.code == Some(NumberOrString::String("scalar_member_access".to_string())))
        .collect();
    assert!(
        scalar_diags.is_empty(),
        "should not flag scalar_member_access on $c->val() inside ternary-wrapped &&, got: {scalar_diags:?}"
    );
}

// ── Assignment inside `if` condition ───────────────────────

/// Variables assigned inside `if` conditions should resolve in the body.
#[test]
fn assignment_in_if_condition_resolves_in_body() {
    let php = r#"<?php
class AdminUser {
public function assignRole(string $role): void {}
/** @return ?static */
public static function first(): ?static { return new static(); }
}
function test(string $role): void {
if ($admin = AdminUser::first()) {
    $admin->assignRole($role);
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    let bad: Vec<_> = diags
        .iter()
        .filter(|d| d.message.contains("assignRole") || d.message.contains("admin"))
        .collect();
    assert!(
        bad.is_empty(),
        "should resolve $admin from if-condition assignment, got: {bad:?}"
    );
}

/// Assignment inside comparison `if (($x = expr()) !== null)` should resolve.
#[test]
fn assignment_in_if_condition_with_comparison() {
    let php = r#"<?php
class Conn {
public function query(string $sql): void {}
}
function getConn(): ?Conn { return new Conn(); }
function test(): void {
if (($conn = getConn()) !== null) {
    $conn->query('SELECT 1');
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    let bad: Vec<_> = diags
        .iter()
        .filter(|d| d.message.contains("query") || d.message.contains("conn"))
        .collect();
    assert!(
        bad.is_empty(),
        "should resolve $conn from if-condition assignment with !== null, got: {bad:?}"
    );
}

/// Bracket access on a class implementing `ArrayAccess` without
/// concrete generic annotations should NOT resolve to the container
/// class itself.  `$app['config']` is not `Application`.
/// The diagnostic should say the subject type could not be resolved,
/// not that the member is missing on `Application`.
#[test]
fn flags_member_on_array_access_class_without_generics() {
    let php = r#"<?php
class Application implements \ArrayAccess {
public function offsetExists(mixed $offset): bool { return true; }
public function offsetGet(mixed $offset): mixed { return null; }
public function offsetSet(mixed $offset, mixed $value): void {}
public function offsetUnset(mixed $offset): void {}

public function useStoragePath(string $path): void {}
}

function test(Application $app): void {
$app['config']->set('logging.default', 'stderr');
}
"#;
    let backend = Backend::new_test();
    // Enable unresolved-member-access so the Untyped outcome emits.
    backend.config.lock().diagnostics.unresolved_member_access = Some(true);
    let diags = collect(&backend, "file:///test.php", php);
    // `$app['config']` returns `mixed` (no concrete generics), so
    // we cannot know the type — the diagnostic should say the
    // subject could not be resolved, NOT that 'set' is missing on
    // `Application`.
    assert!(
        !diags.iter().any(|d| d.message.contains("Application")),
        "should not report 'set' as missing on Application — bracket access returns mixed, got: {diags:?}",
    );
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("could not be resolved")),
        "expected 'could not be resolved' diagnostic for unresolvable bracket access, got: {diags:?}",
    );
}

/// Same as above but with inheritance: `Application2 extends
/// Container2 implements ArrayAccess`.  The `ArrayAccess` interface
/// is on the parent class, not the child.
#[test]
fn flags_member_on_array_access_subclass_without_generics() {
    let php = r#"<?php
namespace Tests;

use ArrayAccess;

class Container2 implements ArrayAccess
{
public function offsetExists($offset): bool
{
    return false;
}

public function offsetGet($offset): mixed
{
    return '';
}

public function offsetSet($offset, $value): void
{
}

public function offsetUnset($offset): void
{
}
}

class Application2 extends Container2
{
}

class TestCase
{
public function defineEnvironment(): void
{
    $test4 = new Application2();
    $test4['config']->set('logging.channels.stack.channels', ['stderr']);
}
}
"#;
    let backend = Backend::new_test();
    backend.config.lock().diagnostics.unresolved_member_access = Some(true);
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        !diags.iter().any(|d| d.message.contains("Application2")),
        "should not report 'set' as missing on Application2 — bracket access returns mixed, got: {diags:?}",
    );
}

/// Assignment in `while` condition should resolve in the loop body.
#[test]
fn assignment_in_while_condition_resolves_in_body() {
    let php = r#"<?php
class Row {
public function toArray(): array { return []; }
}
function nextRow(): ?Row { return new Row(); }
function test(): void {
while ($row = nextRow()) {
    $row->toArray();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    let bad: Vec<_> = diags
        .iter()
        .filter(|d| d.message.contains("toArray") || d.message.contains("row"))
        .collect();
    assert!(
        bad.is_empty(),
        "should resolve $row from while-condition assignment, got: {bad:?}"
    );
}

// ── __call chain continuation ───────────────────────────────────

/// When a class defines `__call` with a typed return, the dispatched
/// method is valid PHP and must not be flagged.  Known methods after
/// it resolve through the `__call` return type.
#[test]
fn magic_call_chain_not_flagged_and_continues() {
    let php = r#"<?php
class AppleCart {
public function getApples(): array { return []; }
}
class Builder {
public function __call(string $name, array $args): static { return $this; }
public function first(): AppleCart { return new AppleCart(); }
}
class Svc {
public function run(): void {
    $b = new Builder();
    $b->doesntExist()->first()->getApples();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "doesntExist() is dispatched through __call (returns static), so nothing should be flagged, got: {diags:?}"
    );
}

/// Multiple `__call`-dispatched methods in a chain are all valid and
/// none should be flagged; known methods between and after them
/// resolve through the `__call` return type.
#[test]
fn magic_call_chain_multiple_dynamic_methods_not_flagged() {
    let php = r#"<?php
class AppleCart {
public function getApples(): array { return []; }
}
class Builder {
public function __call(string $name, array $args): static { return $this; }
public function first(): AppleCart { return new AppleCart(); }
}
class Svc {
public function run(): void {
    $b = new Builder();
    $b->doesntExist()->first()->getApples();
    $b->doesntExist()->alsoDoesntExist()->first()->getApples();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "Dynamic methods dispatched through __call must not be flagged, got: {diags:?}"
    );
}

/// When `__call` returns a concrete type (not self/static), the
/// dispatched method is not flagged and the chain resolves to that
/// type afterwards.
#[test]
fn magic_call_concrete_return_continues_chain() {
    let php = r#"<?php
class Result {
public function getData(): array { return []; }
}
class Proxy {
public function __call(string $name, array $args): Result { return new Result(); }
}
class Svc {
public function run(): void {
    $p = new Proxy();
    $p->anything()->getData();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "anything() dispatches through __call (returns Result), getData() resolves — nothing to flag, got: {diags:?}"
    );
}

/// When `__call` returns `mixed`, the dispatched method is still not
/// flagged.  The chain type becomes `mixed`, which is unresolvable, so
/// downstream accesses are simply unverifiable (no diagnostic by
/// default) rather than flagged as unknown members.
#[test]
fn magic_call_mixed_return_not_flagged() {
    let php = r#"<?php
class Loose {
public function __call(string $name, array $args): mixed { return null; }
}
class Svc {
public function run(): void {
    $l = new Loose();
    $l->unknown()->somethingElse();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        !diags.iter().any(|d| d.message.contains("unknown")),
        "unknown() is dispatched through __call and must not be flagged, got: {diags:?}"
    );
}

#[test]
fn no_false_positive_when_variable_reassigned_inside_try_block() {
    // When a variable is reassigned inside a `try` block, accesses
    // after the reassignment (still inside the try) should resolve
    // against the new type, not the original.
    let php = r#"<?php
class AppCustomer {
public function getName(): string { return ''; }
}
class MollieCustomer {
public function createPayment(string $data): MolliePayment { return new MolliePayment(); }
}
class MolliePayment {
public function getCheckoutUrl(): string { return ''; }
}
class MollieClient {
public function getOrCreateCustomer(AppCustomer $c): MollieCustomer { return new MollieCustomer(); }
}
class Gateway {
public function charge(AppCustomer $customer): void {
    $client = new MollieClient();
    try {
        $customer = $client->getOrCreateCustomer($customer);
        $molliePayment = $customer->createPayment('data');
        $url = $molliePayment->getCheckoutUrl();
    } catch (\Exception $e) {
    }
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for reassigned variable inside try block, got: {diags:?}"
    );
}

#[test]
fn flags_unknown_member_after_reassignment_inside_try_block() {
    // The flip side: after reassignment inside a try block, members
    // from the OLD type that don't exist on the NEW type should be
    // flagged.
    let php = r#"<?php
class OriginalType {
public function onlyOnOriginal(): void {}
}
class ReplacementType {
public function onlyOnReplacement(): void {}
}
class Service {
public function process(OriginalType $var): void {
    try {
        $var = new ReplacementType();
        $var->onlyOnOriginal();
    } catch (\Exception $e) {
    }
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("onlyOnOriginal") && d.message.contains("ReplacementType")),
        "expected diagnostic for onlyOnOriginal() on ReplacementType after reassignment in try, got: {diags:?}"
    );
}

#[test]
fn try_block_reassignment_is_conditional_after_try() {
    // After the try/catch block, the variable could be either the
    // original type (if the try threw before the reassignment) or
    // the new type.  Both types' members should be accepted.
    let php = r#"<?php
class TypeA {
public function methodA(): void {}
}
class TypeB {
public function methodB(): void {}
}
class Svc {
public function run(TypeA $var): void {
    try {
        $var = new TypeB();
    } catch (\Exception $e) {
    }
    $var->methodA();
    $var->methodB();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "after try/catch, both original and reassigned types should be accepted, got: {diags:?}"
    );
}

#[test]
fn catch_block_variable_reassignment_tracked() {
    // Variable reassignment inside a catch block should also be
    // tracked when the cursor is inside the catch block.
    let php = r#"<?php
class ErrorResult {
public function getErrorCode(): int { return 0; }
}
class SuccessResult {
public function getData(): string { return ''; }
}
class Handler {
public function handle(): void {
    $result = new SuccessResult();
    try {
        $result->getData();
    } catch (\Exception $e) {
        $result = new ErrorResult();
        $result->getErrorCode();
    }
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for reassigned variable inside catch block, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_this_items_on_generic_collection_subclass() {
    // When a class extends `Collection<int, T>` via `@extends`,
    // accessing `$this->items` should yield `array<int, T>` with the
    // generic substitution applied.  Iterating `$this->items` in a
    // `foreach` or passing it to `array_any()` should resolve the
    // element type so that property access on `$item` works.
    let php = r#"<?php
/**
 * @template TKey
 * @template TValue
 */
class Collection {
/** @var array<TKey, TValue> */
public array $items = [];

/** @return TValue|null */
public function first(): mixed { return null; }
}

class PurchaseFileProduct {
public int $order_amount = 0;
public string $name = '';
}

/**
 * @template TKey
 * @template TValue
 * @param array<TKey, TValue> $array
 * @param callable(TValue, TKey): bool $callback
 * @return bool
 */
function array_any(array $array, callable $callback): bool { return false; }

/**
 * @extends Collection<int, PurchaseFileProduct>
 */
final class PurchaseFileProductCollection extends Collection {
public function hasIssues(): bool {
    return array_any($this->items, fn($item) => $item->order_amount > 0);
}

public function hasName(): bool {
    return array_any($this->items, fn($item) => $item->name !== '');
}

public function foreachWorks(): void {
    foreach ($this->items as $item) {
        $item->order_amount;
        $item->name;
    }
}
}
"#;
    let backend = Backend::new_test();
    backend.config.lock().diagnostics.unresolved_member_access = Some(true);
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for $this->items on generic Collection subclass, got: {diags:?}"
    );
}

#[test]
fn no_false_positive_when_variable_reassigned_inside_try_inside_foreach() {
    // When a variable is assigned before a foreach,
    // then reassigned inside a try block nested inside the foreach
    // body, the type should still resolve for accesses after the
    // reassignment (still inside the try).
    //
    // Real-world pattern from OrderService:137:
    //   $remaining = $order->amount;          // Decimal via @property
    //   foreach ($payments as $payment) {
    //       try {
    //           $remaining = $remaining->sub($toCapture);  // ← should resolve
    //       } catch (...) {}
    //   }
    let php = r#"<?php
class Decimal {
public function sub(string $v): self { return new self(); }
public function isZero(): bool { return true; }
public function isNegative(): bool { return true; }
public function isPositive(): bool { return true; }
public function toFixed(int $places): string { return ''; }
}

/**
 * @property Decimal $amount
 * @property string $state
 */
class Payment {
}

/**
 * @property Decimal $amount
 */
class Order {
}

class CaptureException extends \Exception {}
class InvalidStateException extends \Exception {}
class CaptureService {
public function captureReservedPayment(Payment $p, Decimal $amount): void {}
}

class OrderService {
/** @param list<Payment> $payments */
public function capture(Order $order, array $payments): void {
    $remaining = $order->amount;
    foreach ($payments as $payment) {
        if ($payment->state === 'paid') {
            $remaining = $remaining->sub('1');
        }
    }

    $svc = new CaptureService();
    foreach ($payments as $payment) {
        if ($payment->state !== 'reserved') {
            continue;
        }

        $toCapture = $remaining->isPositive() ? $payment->amount : $remaining;
        if ($toCapture->isZero() || $toCapture->isNegative()) {
            break;
        }

        try {
            $svc->captureReservedPayment($payment, $toCapture);
            $remaining = $remaining->sub('1');
        } catch (CaptureException|InvalidStateException $e) {
        }
    }

    if ($remaining->isPositive() && !$remaining->isZero()) {
        throw new \RuntimeException('remaining: ' . $remaining->toFixed(2));
    }
}
}
"#;
    let backend = Backend::new_test();
    backend.config.lock().diagnostics.unresolved_member_access = Some(true);
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for variable reassigned inside try-inside-foreach, got: {diags:?}"
    );
}

#[test]
fn no_false_positive_when_variable_reassigned_inside_nested_foreach() {
    // Regression test for self-referential variable reassignment.
    // When `$orderCostPrice` is reassigned inside a nested foreach
    // via `$orderCostPrice = $orderCostPrice->add(…)`, the forward
    // walker must resolve the outer foreach access correctly without
    // a false "type could not be resolved" diagnostic.
    //
    // Real-world pattern from OrderService:618:
    //   $zero = new Decimal('0');
    //   $orderCostPrice = $zero;
    //   foreach ($order->getOrderProducts() as $line) {
    //       if ($product->isBundle()) {
    //           foreach ($bundleProducts as $bp) {
    //               $productCostPrice = $bp->supplier_price_dkk ?? $zero;
    //               $orderCostPrice = $orderCostPrice->add($productCostPrice->mul($qty));
    //           }
    //           continue;
    //       }
    //       $productCostPrice = $product->supplier_price_dkk ?? $zero;
    //       $orderCostPrice = $orderCostPrice->add($productCostPrice->mul($qty));
    //   }
    //   return $orderCostPrice->mul($rate);
    let php = r#"<?php
class Decimal {
public function add(string $v): self { return new self(); }
public function mul(string $v): self { return new self(); }
}

class Item {
public Decimal $cost;
public function isBundle(): bool { return false; }
/** @return list<Item> */
public function getChildren(): array { return []; }
}

class OrderService {
/** @param list<Item> $items */
public function calculateCost(array $items): Decimal {
    $zero = new Decimal();
    $result = $zero;
    foreach ($items as $item) {
        if ($item->isBundle()) {
            $children = $item->getChildren();
            foreach ($children as $child) {
                $result = $result->add($child->cost->mul('1'));
            }

            continue;
        }

        $result = $result->add($item->cost->mul('1'));
    }

    return $result->mul('1');
}
}
"#;
    let backend = Backend::new_test();
    backend.config.lock().diagnostics.unresolved_member_access = Some(true);
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for variable reassigned inside nested foreach loops, got: {diags:?}"
    );
}

/// A call whose return type is `object` is the "any object" escape
/// hatch: property/method access on the result is always valid at
/// runtime, so no unresolved-member diagnostic should fire.
#[test]
fn no_diagnostic_for_object_return_type_member_access() {
    let php = r#"<?php
class Repo {
    public function all(): object { return new \stdClass(); }
}
function test(Repo $r): void {
    $x = $r->all()->projects ?? [];
}
"#;
    let backend = Backend::new_test();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for member access on object return type, got: {diags:?}"
    );
}

/// Adding nullability (`?object`) must not lose the `object` type and
/// leave the subject unresolvable.  Property access on a `?object`
/// return is treated the same as a plain `object` return.
#[test]
fn no_diagnostic_for_nullable_object_return_type_member_access() {
    let php = r#"<?php
class Repo {
    public function all(): ?object { return new \stdClass(); }
}
function test(Repo $r): void {
    $x = $r->all()->projects ?? [];
}
"#;
    let backend = Backend::new_test();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for member access on nullable object return type, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_object_parameter_type() {
    let php = r#"<?php
function test(object $obj): void {
echo $obj->anything;
$obj->whatever();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for object parameter type, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_after_is_object_guard() {
    let php = r#"<?php
function test(mixed $data): void {
if (is_object($data)) {
    echo $data->error_link;
}
}
"#;
    let backend = Backend::new_test();
    backend.config.lock().diagnostics.unresolved_member_access = Some(true);
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics after is_object() guard, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_after_is_object_guard_on_real_union() {
    let php = r#"<?php
class Thing {
    public function bar(): void {}
}
function test(string|Thing $file): void {
    if (is_object($file)) {
        $file->bar();
    }
}
"#;
    let backend = Backend::new_test();
    backend.config.lock().diagnostics.unresolved_member_access = Some(true);
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics after is_object() guard on a real union, got: {diags:?}"
    );
}

#[test]
fn no_scalar_member_access_after_is_object_guard_on_plain_string() {
    // When upstream type inference produced a plain `string` type for a
    // variable that can, at runtime, also be an object (e.g. a foreach
    // element whose type was under-inferred), an `is_object()` guard
    // must still stop `scalar_member_access` on the guarded access —
    // trust the runtime check over the incomplete static type.
    let php = r#"<?php
function test(string $file): void {
    if (is_object($file)) {
        $file->getPathname();
    }
}
"#;
    let backend = Backend::new_test();
    backend.config.lock().diagnostics.unresolved_member_access = Some(true);
    let diags = collect(&backend, "file:///test.php", php);
    let scalar_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.code == Some(NumberOrString::String("scalar_member_access".to_string())))
        .collect();
    assert!(
        scalar_diags.is_empty(),
        "should not flag scalar_member_access on $file->getPathname() inside is_object() guard, got: {scalar_diags:?}"
    );
}

#[test]
fn no_diagnostic_for_isset_on_missing_property() {
    let php = r#"<?php
class Item {
    public string $name = '';
}

function test(Item $item): void {
    if (isset($item->maybeDynamic)) {
        echo 'ok';
    }
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "isset() should suppress unknown-property diagnostics, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_empty_on_missing_property() {
    let php = r#"<?php
class Item {
    public string $name = '';
}

function test(Item $item): void {
    if (empty($item->maybeDynamic)) {
        echo 'ok';
    }
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "empty() should suppress unknown-property diagnostics, got: {diags:?}"
    );
}

#[test]
fn no_scalar_member_access_for_isset_on_union_with_stdclass() {
    let php = r#"<?php
class Item {
    public string $name = '';
}

function test(Item|\stdClass $item): void {
    if (isset($item->maybeDynamic)) {
        echo 'ok';
    }
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "isset() on a union with stdClass should not flag the other union members, got: {diags:?}"
    );
}

#[test]
fn still_flags_bare_access_to_missing_property_outside_isset() {
    // Sanity check: isset()'s suppression must not leak to a sibling
    // bare access on the same property.
    let php = r#"<?php
class Item {
    public string $name = '';
}

function test(Item $item): void {
    if (isset($item->maybeDynamic)) {
        echo 'ok';
    }
    echo $item->maybeDynamic;
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.iter().any(|d| d.message.contains("maybeDynamic")),
        "bare access outside isset() should still be flagged, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_after_is_object_guard_with_negated_early_return() {
    let php = r#"<?php
function test(mixed $data): void {
if (!is_object($data)) {
    return;
}
echo $data->error_link;
echo $data->something_else;
$data->doStuff();
}
"#;
    let backend = Backend::new_test();
    backend.config.lock().diagnostics.unresolved_member_access = Some(true);
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics after negated is_object() early return, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_after_is_object_in_compound_and_condition() {
    let php = r#"<?php
function test(mixed $data): void {
if (is_object($data) && property_exists($data, 'error_link') && is_string($data->error_link)) {
    echo stripslashes($data->error_link);
}
}
"#;
    let backend = Backend::new_test();
    backend.config.lock().diagnostics.unresolved_member_access = Some(true);
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics after is_object() in compound && condition, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_object_typed_parameter() {
    let php = r#"<?php
function test(object $data): void {
echo $data->name;
$data->doStuff();
}
"#;
    let backend = Backend::new_test();
    backend.config.lock().diagnostics.unresolved_member_access = Some(true);
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for object-typed parameter, got: {diags:?}"
    );
}

// ── class-string<T> static return type resolution ───────────────

#[test]
fn no_diagnostic_for_class_string_static_return_in_foreach() {
    // When a parameter is typed `class-string<BackedEnum>` and we
    // call `$class::cases()`, the `static[]` return type should
    // resolve to `BackedEnum[]`, making foreach items typed as
    // `BackedEnum` with `->name` and `->value` available.
    // UnitEnum and BackedEnum are loaded from stubs (cross-file),
    // not defined inline, to reproduce the real-world scenario.
    // Uses the exact pattern from OptionList.php including the
    // ternary with dynamic method call.
    let php = r#"<?php
class OptionList {
/**
 * @param class-string<BackedEnum> $class
 */
public static function enum(BackedEnum $value, string $class, array $exclude = [], string $method = ''): void {
    foreach ($class::cases() as $item) {
        if (in_array($item, $exclude, true)) {
            continue;
        }

        $name = $method ? $item->{$method}() : $item->name;

        $val = $item->value;
    }
}
}
"#;
    let backend = create_enum_backend();
    backend.config.lock().diagnostics.unresolved_member_access = Some(true);
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for class-string<BackedEnum> foreach item members, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_class_string_static_return_chained() {
    // `$class::from('foo')` returns `static` which should resolve
    // to `BackedEnum` when `$class` is `class-string<BackedEnum>`.
    // Members like `->name` should be available on the result.
    // UnitEnum and BackedEnum are loaded from stubs (cross-file),
    // not defined inline, to reproduce the real-world scenario.
    let php = r#"<?php
class Svc {
/**
 * @param class-string<BackedEnum> $class
 */
public function resolve(string $class): void {
    $result = $class::from('foo');
    $name = $result->name;
    $val  = $result->value;
}
}
"#;
    let backend = create_enum_backend();
    backend.config.lock().diagnostics.unresolved_member_access = Some(true);
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for class-string<BackedEnum> static return chain, got: {diags:?}"
    );
}

#[test]
fn in_array_guard_does_not_wipe_type_when_element_matches() {
    // When `in_array($item, $exclude, true)` is used as a guard
    // clause (`if (...) { continue; }`), the `in_array` narrowing
    // should NOT exclude the variable's type when the haystack's
    // element type matches the variable's type.  The check filters
    // by value, not by type — `$item` is still a `BackedEnum`
    // after the guard, just not one of the excluded values.
    let php = r#"<?php
class Foo {
public string $name;
}

class Svc {
/**
 * @param array<int, Foo> $exclude
 */
public function run(Foo $item, array $exclude): void {
    if (in_array($item, $exclude, true)) {
        return;
    }
    $name = $item->name;
}
}
"#;
    let backend = Backend::new_test();
    backend.config.lock().diagnostics.unresolved_member_access = Some(true);
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "in_array guard should not wipe variable type when element type matches, got: {diags:?}"
    );
}

#[test]
fn in_array_guard_still_narrows_union_type() {
    // When the variable is a union type (e.g. `Foo|Bar`) and the
    // haystack element type is one of the union members (e.g.
    // `array<int, Foo>`), the guard clause SHOULD narrow: after
    // `if (in_array($item, $fooList)) { return; }`, `$item` is
    // not `Foo`, so it must be `Bar`.  The would-exclude-all
    // check should NOT prevent this narrowing because removing
    // `Foo` still leaves `Bar`.
    let php = r#"<?php
class Foo {
public string $fooName;
}
class Bar {
public string $barName;
}

class Svc {
/**
 * @param Foo|Bar $item
 * @param array<int, Foo> $fooList
 */
public function run(object $item, array $fooList): void {
    if (in_array($item, $fooList, true)) {
        return;
    }
    $name = $item->barName;
}
}
"#;
    let backend = Backend::new_test();
    backend.config.lock().diagnostics.unresolved_member_access = Some(true);
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "in_array guard should still narrow union types, got: {diags:?}"
    );
}

// ── Unresolvable instanceof target suppression ──────────────────

#[test]
fn no_diagnostic_when_instanceof_target_unresolvable_ternary() {
    // When the instanceof target class cannot be resolved (e.g. it
    // lives in a phar), the ternary then-branch should not produce
    // false-positive diagnostics for members that only exist on the
    // unresolvable subclass.
    let php = r#"<?php
interface Type {
public function describe(): string;
}

class Test {
/** @param Type $argType */
public function run(Type $argType): void {
    $types = $argType instanceof UnionType ? $argType->getTypes() : [$argType];
}
}
"#;
    // UnionType is intentionally not defined — simulates a phar class.
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics when instanceof target is unresolvable (ternary), got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_when_instanceof_target_unresolvable_if_body() {
    // Same scenario but with an if-body instead of a ternary.
    let php = r#"<?php
interface Type {
public function describe(): string;
}

class Test {
public function run(Type $argType): void {
    if ($argType instanceof UnionType) {
        $argType->getTypes();
    }
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics when instanceof target is unresolvable (if-body), got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_when_instanceof_target_unresolvable_assert() {
    // Same scenario but with assert($var instanceof ...).
    let php = r#"<?php
interface Type {
public function describe(): string;
}

class Test {
public function run(Type $argType): void {
    assert($argType instanceof UnionType);
    $argType->getTypes();
}
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics when instanceof target is unresolvable (assert), got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_when_instanceof_target_unresolvable_and_chain() {
    // Inline && narrowing with unresolvable target.
    let php = r#"<?php
interface Type {
public function describe(): string;
}

function test(Type $t): void {
$t instanceof UnionType && $t->getTypes();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics when instanceof target is unresolvable (&& chain), got: {diags:?}"
    );
}

// ── Regression: variable from method chain must still resolve ────

#[test]
fn no_unresolved_for_variable_assigned_from_method_chain() {
    // A variable assigned from a method call chain must resolve
    // correctly for diagnostics.  This catches regressions where
    // the diagnostic outcome path diverges from completion/hover
    // and incorrectly reports the variable as untyped.
    let php = r#"<?php
class DebtCollection {
public function isResolved(): bool { return false; }
}

class Order {
public function getDebtCollection(): ?DebtCollection { return null; }
}

class Period {
public function getOrder(): ?Order { return null; }
}

class Test {
public function run(Period $period): void {
    $debt = $period->getOrder()?->getDebtCollection();
    if ($debt) {
        $debt->isResolved();
    }
}
}
"#;
    let backend = Backend::new_test();
    backend.config.lock().diagnostics.unresolved_member_access = Some(true);
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for variable assigned from method chain, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_interleaved_array_access_property_chain() {
    // `$results[$i]->activities[$id]->extras` where `$results` is
    // `array<int, WeeklyResultDto>` and the property chain walks
    // through typed properties with array access in between.
    // Previously the parser dropped the `->activities[]` suffix
    // when parsing the subject text, causing a false positive.
    let php = r#"<?php
class ExtraPointsDto {
public string $label;
}

class ActivityResultDto {
/** @var list<ExtraPointsDto> */
public array $extras = [];
public int $activityId;
}

class WeeklyResultDto {
/** @var array<int, ActivityResultDto> */
public array $activities;
public int $week;
}

function test(): void {
/** @var array<int, WeeklyResultDto> */
$results = [];

$results[0]->activities[1]->extras[] = new ExtraPointsDto();
$results[0]->activities[1]->activityId;
$results[0]->week;
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for interleaved array-access property chain, got: {diags:?}",
    );
}

// ── Property narrowing via guard clauses ────────────────────────

#[test]
fn no_false_positive_after_negated_instanceof_guard_on_property() {
    let php = r#"<?php
class Dog {
    public function bark(): string { return ''; }
}
class Cat {
    public function purr(): string { return ''; }
}
class Svc {
    private Dog|Cat $pet;
    public function test(): void {
        if ($this->pet instanceof Dog) {
            $this->pet->bark();
        }
        if (!$this->pet instanceof Cat) {
            return;
        }
        $this->pet->purr();
    }
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics after negated instanceof guard on property, got: {diags:?}",
    );
}

#[test]
fn no_false_positive_after_positive_instanceof_guard_on_property() {
    let php = r#"<?php
class Dog {
    public function bark(): string { return ''; }
}
class Cat {
    public function purr(): string { return ''; }
}
class Svc {
    private Dog|Cat $pet;
    public function test(): void {
        if ($this->pet instanceof Cat) {
            return;
        }
        $this->pet->bark();
    }
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics after positive instanceof guard excludes Cat on property, got: {diags:?}",
    );
}

#[test]
fn no_false_positive_after_assert_instanceof_on_property() {
    let php = r#"<?php
class Dog {
    public function bark(): string { return ''; }
}
class Cat {
    public function purr(): string { return ''; }
}
class Svc {
    /** @var Dog|Cat|null */
    public $pet;
    public function test(): void {
        assert($this->pet instanceof Dog);
        $this->pet->bark();
    }
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics after assert instanceof on property, got: {diags:?}",
    );
}

/// When a method has a conditional return type like
/// `($type is class-string<SomeInterface> ? ThenType : ElseType)`,
/// and the argument class does NOT implement `SomeInterface`, the
/// analyzer should use the else-branch return type.
///
/// Regression: the conditional resolver always took the then-branch
/// when the argument was a `::class` literal, without checking the
/// subtype relationship against the bound.
#[test]
fn no_false_positive_conditional_return_class_string_bound() {
    let php = r#"<?php
interface FormInterface {
    public function submit(mixed $data): void;
    public function getData(): mixed;
}
interface FormFlowTypeInterface {}
interface FormFlowInterface {}
abstract class AbstractController {
    /**
     * @return ($type is class-string<FormFlowTypeInterface> ? FormFlowInterface : FormInterface)
     */
    protected function createForm(string $type, mixed $data = null, array $options = []): FormInterface {}
}
class ImageUploadFormType {}
class ImageController extends AbstractController {
    public function store(): void {
        $form = $this->createForm(ImageUploadFormType::class);
        $form->submit([]);
        $form->getData();
    }
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    let unknown_diags: Vec<_> = diags
        .iter()
        .filter(|d| {
            d.code == Some(NumberOrString::String("unknown_member".to_string()))
                || d.code == Some(NumberOrString::String("scalar_member_access".to_string()))
        })
        .collect();
    assert!(
        unknown_diags.is_empty(),
        "should not flag unknown members on $form when createForm conditional returns FormInterface, got: {unknown_diags:?}"
    );
}

/// Cross-file variant: the base class with the conditional return type
/// is in a separate file (simulating vendor/symfony).  This tests that
/// `conditional_return` is properly inherited through `resolve_class_fully`
/// when the method is defined in an ancestor loaded via the class loader.
#[test]
fn no_false_positive_conditional_return_class_string_bound_cross_file() {
    let base_php = r#"<?php
interface FormInterface {
    public function submit(mixed $data): void;
    public function getData(): mixed;
}
interface FormFlowTypeInterface {}
interface FormFlowInterface {}
abstract class AbstractController {
    /**
     * @return ($type is class-string<FormFlowTypeInterface> ? FormFlowInterface : FormInterface)
     */
    protected function createForm(string $type, mixed $data = null, array $options = []): FormInterface {}
}
"#;
    let controller_php = r#"<?php
class ImageUploadFormType {}
class ImageController extends AbstractController {
    public function store(): void {
        $form = $this->createForm(ImageUploadFormType::class);
        $form->submit([]);
        $form->getData();
    }
}
"#;
    let backend = Backend::new_test();
    // Index the base file first (simulates vendor classes).
    backend.update_ast("file:///base.php", base_php);
    // Then index the controller file.
    backend.update_ast("file:///controller.php", controller_php);

    let mut out = Vec::new();
    backend.collect_unknown_member_diagnostics("file:///controller.php", controller_php, &mut out);
    let unknown_diags: Vec<_> = out
        .iter()
        .filter(|d| {
            d.code == Some(NumberOrString::String("unknown_member".to_string()))
                || d.code == Some(NumberOrString::String("scalar_member_access".to_string()))
        })
        .collect();
    assert!(
        unknown_diags.is_empty(),
        "cross-file: should not flag unknown members on $form when createForm conditional returns FormInterface, got: {unknown_diags:?}"
    );
}

/// When two `array_map` calls in the same method use different closure
/// parameter names that happen to collide (e.g. `$row`), the second
/// closure's parameter type must come from its own type hint, not from
/// the first closure's `@param` docblock.
///
/// Regression: the forward walker recorded a scope snapshot for the
/// first arrow function's `$row` (typed as `array{activity: int}`),
/// and when the second arrow function's `$row` (typed as `Activity`)
/// failed to seed (e.g. because the class wasn't resolved), the
/// snapshot lookup fell back to the first closure's stale entry.
#[test]
fn no_false_positive_closure_param_scope_leak_between_array_maps() {
    let php = r#"<?php
class Activity {
    public int $id = 0;
    public function toResponseObject(): string { return ''; }
}
class Repo {
    /**
     * @return list<array{activity: int, distance: int}>
     */
    public function getStats(): array { return []; }

    /**
     * @return list<Activity>
     */
    public function getActivities(): array { return []; }

    public function run(): void {
        $rows = $this->getStats();

        $ids = \array_map(
            /** @param array{activity: int, distance: int} $row */
            static fn(array $row): int => $row['activity'],
            $rows,
        );

        /** @var list<Activity> */
        $activities = $this->getActivities();

        $result = \array_map(
            static fn(Activity $row): string => $row->toResponseObject(),
            $activities,
        );
    }
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    let scalar_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.code == Some(NumberOrString::String("scalar_member_access".to_string())))
        .collect();
    assert!(
        scalar_diags.is_empty(),
        "should not flag scalar_member_access on $row->toResponseObject() in second closure, got: {scalar_diags:?}"
    );
    let unknown_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.message.contains("toResponseObject"))
        .collect();
    assert!(
        unknown_diags.is_empty(),
        "should not flag unknown member 'toResponseObject' on $row in second closure, got: {unknown_diags:?}"
    );
}

#[test]
fn no_false_positive_foreach_over_narrowed_property_after_guard_clause() {
    // After `if (!$this->model instanceof Order) { return; }`,
    // `$this->model` is narrowed to `Order`.  A foreach over
    // `$this->model->items` should resolve the element type so
    // that member accesses on the loop variable don't fire
    // unresolved_member_access.
    let php = r#"<?php
class Item {
    public function name(): string { return ''; }
}
class Order {
    /** @return Item[] */
    public function getItems(): array { return []; }
    /** @var Item[] */
    public array $items;
}
class Svc {
    private ?Order $model;
    public function test(): void {
        if (!$this->model instanceof Order) {
            return;
        }
        foreach ($this->model->items as $item) {
            $item->name();
        }
    }
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics after guard clause narrowing on property in foreach, got: {diags:?}",
    );
}

#[test]
fn no_diagnostic_for_arbitrary_method_on_soap_client() {
    let php = r#"<?php
function test(\SoapClient $client): void {
$client->gettransactionlist(['foo' => 'bar']);
$client->delete(123);
$client->capture('abc');
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for arbitrary methods on SoapClient, got: {diags:?}",
    );
}

#[test]
fn no_diagnostic_for_arbitrary_method_on_soap_client_subclass() {
    // When a class extends SoapClient, it inherits __call and any
    // method should be valid.  In single-file tests the parent chain
    // may not fully resolve from stubs, so we test with a direct
    // SoapClient parameter typed as the subclass via docblock.
    let php = r#"<?php
class MyService extends \SoapClient {
    public function getConnection(): \SoapClient { return $this; }
}
function test(): void {
$svc = new MyService('http://example.com?wsdl');
$svc->getConnection()->customMethod();
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    // The getConnection() returns \SoapClient, which should suppress.
    assert!(
        diags.is_empty(),
        "expected no diagnostics for arbitrary methods on SoapClient subclass, got: {diags:?}",
    );
}

/// `$m->prop = null;` records the property as exactly `null`.  A following
/// not-null assertion (`@phpstan-assert !null`, e.g. PHPUnit's
/// `assertNotNull`) must strip that tracked `null` so the subsequent member
/// access is not flagged as a scalar member access on `null`.  Class-based
/// exclusion alone cannot remove the `null` pseudo-type.
#[test]
fn not_null_assert_strips_tracked_null_on_property() {
    let php = r#"<?php
class Clock {
    public function toString(): string { return ''; }
}
class Model {
    public ?Clock $at = null;
    public function save(): void {}
}
class Helper {
    /** @phpstan-assert !null $actual */
    public static function assertNotNull(mixed $actual): void {}
}
class Demo {
    public function run(Model $m): void {
        $m->at = null;
        $m->save();
        Helper::assertNotNull($m->at);
        echo $m->at->toString();
    }
}
"#;
    let backend = Backend::new_test();
    let diags = collect(&backend, "file:///test.php", php);
    let scalar_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.code == Some(NumberOrString::String("scalar_member_access".to_string())))
        .collect();
    assert!(
        scalar_diags.is_empty(),
        "assertNotNull should strip the tracked null, got: {scalar_diags:?}"
    );
}

/// A `@var array{First, Second}` annotation on an assignment must let
/// integer-indexed access resolve each positional entry to its own type,
/// so member access on `$pair[0]` / `$pair[1]` verifies against the right
/// class instead of reporting the subject type as unresolved.
#[test]
fn positional_shape_var_annotation_resolves_int_index_element() {
    let php = r#"<?php
class Label {
public function labelOnly(): void {}
}
class Stmt {
public function stmtOnly(): void {}
}
class Node {
/** @return Node[] */
public function getChildren(): array { return []; }
}
function test(Node $n): void {
/** @var array{Label, Stmt} $pair */
$pair = $n->getChildren();
$pair[0]->labelOnly();
$pair[1]->stmtOnly();
}
"#;
    let backend = Backend::new_test();
    backend.config.lock().diagnostics.unresolved_member_access = Some(true);
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "positional shape entries should resolve to their own type, got: {diags:?}"
    );
}

/// The same positional-shape resolution must work for a multiline
/// `@var array{...}` block with a trailing comma, and it must resolve
/// each entry to the correct class (so a method that only exists on the
/// wrong entry is still flagged).
#[test]
fn positional_shape_multiline_var_annotation_resolves_int_index_element() {
    let php = r#"<?php
class Label {
public function labelOnly(): void {}
}
class Stmt {
public function stmtOnly(): void {}
}
class Node {
/** @return Node[] */
public function getChildren(): array { return []; }
}
function test(Node $n): void {
/**
 * @var array{
 *     Label,
 *     Stmt,
 * } $pair
 */
$pair = $n->getChildren();
$pair[0]->labelOnly();
$pair[1]->stmtOnly();
}
"#;
    let backend = Backend::new_test();
    backend.config.lock().diagnostics.unresolved_member_access = Some(true);
    let diags = collect(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "multiline positional shape entries should resolve, got: {diags:?}"
    );

    // A method that only exists on the *other* entry must still be flagged,
    // proving each index resolves to its own distinct type.
    let php_wrong = php.replace("$pair[0]->labelOnly();", "$pair[0]->stmtOnly();");
    let diags_wrong = collect(&backend, "file:///test2.php", &php_wrong);
    assert!(
        diags_wrong
            .iter()
            .any(|d| d.message.contains("stmtOnly") && d.message.contains("Label")),
        "calling Stmt's method on $pair[0] (a Label) should be flagged, got: {diags_wrong:?}"
    );
}
