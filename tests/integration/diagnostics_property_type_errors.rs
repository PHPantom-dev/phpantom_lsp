use crate::common::create_test_backend;
use tower_lsp::lsp_types::*;

// ─── Helpers ────────────────────────────────────────────────────────────────

fn collect(php: &str) -> Vec<Diagnostic> {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    backend.update_ast(uri, php);
    let mut out = Vec::new();
    backend.collect_property_type_diagnostics(uri, php, &mut out);
    out
}

fn has_property_error(diags: &[Diagnostic]) -> bool {
    diags.iter().any(|d| {
        d.code.as_ref().is_some_and(
            |c| matches!(c, NumberOrString::String(s) if s == "type_mismatch_property"),
        )
    })
}

fn property_error_messages(diags: &[Diagnostic]) -> Vec<String> {
    diags
        .iter()
        .filter(|d| {
            d.code.as_ref().is_some_and(
                |c| matches!(c, NumberOrString::String(s) if s == "type_mismatch_property"),
            )
        })
        .map(|d| d.message.clone())
        .collect()
}

// ─── Basic: assign wrong type to property ───────────────────────────────────

#[test]
fn flags_string_assigned_to_int_property() {
    let php = r#"<?php
class Foo {
    public int $count;

    public function set(): void {
        $this->count = "hello";
    }
}
"#;
    let diags = collect(php);
    assert!(
        has_property_error(&diags),
        "Expected property type error for string assigned to int, got: {diags:?}"
    );
    let msgs = property_error_messages(&diags);
    assert!(
        msgs.iter().any(|m| m.contains("count")),
        "Expected message mentioning property name, got: {msgs:?}"
    );
}

// ─── Correct assignment — no diagnostic ────────────────────────────────────

#[test]
fn no_diagnostic_for_correct_property_assignment() {
    let php = r#"<?php
class Foo {
    public string $name;

    public function set(): void {
        $this->name = "hello";
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag correct property assignment, got: {diags:?}"
    );
}

// ─── Assign null to non-nullable property ──────────────────────────────────

#[test]
fn flags_null_assigned_to_non_nullable_property() {
    let php = r#"<?php
class Foo {
    public string $name;

    public function clear(): void {
        $this->name = null;
    }
}
"#;
    let diags = collect(php);
    assert!(
        has_property_error(&diags),
        "Expected property type error for null assigned to string, got: {diags:?}"
    );
}

// ─── Assign null to nullable property — OK ─────────────────────────────────

#[test]
fn no_diagnostic_for_null_to_nullable_property() {
    let php = r#"<?php
class Foo {
    public ?string $name;

    public function clear(): void {
        $this->name = null;
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag null assigned to ?string, got: {diags:?}"
    );
}

// ─── Assign int to string property ─────────────────────────────────────────

#[test]
fn flags_array_assigned_to_string_property_basic() {
    let php = r#"<?php
class Config {
    public string $label;

    public function update(): void {
        $this->label = [];
    }
}
"#;
    let diags = collect(php);
    assert!(
        has_property_error(&diags),
        "Expected property type error for array assigned to string, got: {diags:?}"
    );
}

// ─── Assign bool to int property ───────────────────────────────────────────

#[test]
fn flags_bool_assigned_to_int_property() {
    let php = r#"<?php
class Counter {
    public int $value;

    public function reset(): void {
        $this->value = true;
    }
}
"#;
    let diags = collect(php);
    assert!(
        has_property_error(&diags),
        "Expected property type error for bool assigned to int, got: {diags:?}"
    );
}

// ─── Untyped property — no diagnostic ──────────────────────────────────────

#[test]
fn no_diagnostic_for_untyped_property() {
    let php = r#"<?php
class Foo {
    public $anything;

    public function set(): void {
        $this->anything = 42;
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag untyped property, got: {diags:?}"
    );
}

// ─── Mixed property — no diagnostic ────────────────────────────────────────

#[test]
fn no_diagnostic_for_mixed_property() {
    let php = r#"<?php
class Foo {
    public mixed $data;

    public function set(): void {
        $this->data = "hello";
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag mixed property, got: {diags:?}"
    );
}

// ─── Union property type — compatible ──────────────────────────────────────

#[test]
fn no_diagnostic_for_union_property_correct() {
    let php = r#"<?php
class Foo {
    public string|int $value;

    public function set(): void {
        $this->value = 42;
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag int assigned to string|int, got: {diags:?}"
    );
}

// ─── Union property type — incompatible ────────────────────────────────────

#[test]
fn flags_bool_assigned_to_string_or_int_property() {
    let php = r#"<?php
class Foo {
    public string|int $value;

    public function set(): void {
        $this->value = true;
    }
}
"#;
    let diags = collect(php);
    assert!(
        has_property_error(&diags),
        "Expected property type error for bool assigned to string|int, got: {diags:?}"
    );
}

// ─── Array assigned to string property ─────────────────────────────────────

#[test]
fn flags_array_assigned_to_string_property() {
    let php = r#"<?php
class Foo {
    public string $name;

    public function set(): void {
        $this->name = [];
    }
}
"#;
    let diags = collect(php);
    assert!(
        has_property_error(&diags),
        "Expected property type error for array assigned to string, got: {diags:?}"
    );
}

// ─── Compound assignment (+=) — not flagged ────────────────────────────────

#[test]
fn no_diagnostic_for_compound_assignment() {
    let php = r#"<?php
class Counter {
    public int $count = 0;

    public function increment(): void {
        $this->count += 1;
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag compound assignment (+=), got: {diags:?}"
    );
}

// ─── Assignment inside if/else ─────────────────────────────────────────────

#[test]
fn flags_wrong_assignment_in_if_branch() {
    let php = r#"<?php
class Foo {
    public int $count;

    public function set(bool $flag): void {
        if ($flag) {
            $this->count = "wrong";
        } else {
            $this->count = 42;
        }
    }
}
"#;
    let diags = collect(php);
    let msgs = property_error_messages(&diags);
    assert_eq!(
        msgs.len(),
        1,
        "Expected exactly one property error (in if branch), got: {msgs:?}"
    );
}

// ─── Assignment in constructor ─────────────────────────────────────────────

#[test]
fn flags_wrong_assignment_in_constructor() {
    let php = r#"<?php
class User {
    public string $name;

    public function __construct() {
        $this->name = [];
    }
}
"#;
    let diags = collect(php);
    assert!(
        has_property_error(&diags),
        "Expected property type error in constructor, got: {diags:?}"
    );
}

// ─── Correct constructor assignment — no diagnostic ────────────────────────

#[test]
fn no_diagnostic_for_correct_constructor_assignment() {
    let php = r#"<?php
class User {
    public string $name;

    public function __construct(string $name) {
        $this->name = $name;
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag correct constructor assignment, got: {diags:?}"
    );
}

// ─── Assignment in try/catch ───────────────────────────────────────────────

#[test]
fn flags_wrong_assignment_in_try() {
    let php = r#"<?php
class Loader {
    public string $result;

    public function load(): void {
        try {
            $this->result = false;
        } catch (\Exception $e) {
            $this->result = "error";
        }
    }
}
"#;
    let diags = collect(php);
    let msgs = property_error_messages(&diags);
    assert_eq!(
        msgs.len(),
        1,
        "Expected one property error (in try block), got: {msgs:?}"
    );
}

// ─── Multiple wrong assignments ────────────────────────────────────────────

#[test]
fn flags_multiple_wrong_assignments() {
    let php = r#"<?php
class Foo {
    public int $a;
    public string $b;

    public function set(): void {
        $this->a = "wrong";
        $this->b = [];
    }
}
"#;
    let diags = collect(php);
    let msgs = property_error_messages(&diags);
    assert_eq!(msgs.len(), 2, "Expected two property errors, got: {msgs:?}");
}

// ─── Assignment inside foreach ─────────────────────────────────────────────

#[test]
fn flags_wrong_assignment_in_foreach() {
    let php = r#"<?php
class Aggregator {
    public int $total;

    public function aggregate(array $items): void {
        foreach ($items as $item) {
            $this->total = "wrong";
        }
    }
}
"#;
    let diags = collect(php);
    assert!(
        has_property_error(&diags),
        "Expected property type error in foreach, got: {diags:?}"
    );
}

// ─── Assignment inside while ───────────────────────────────────────────────

#[test]
fn flags_wrong_assignment_in_while() {
    let php = r#"<?php
class Runner {
    public bool $running;

    public function run(): void {
        while (true) {
            $this->running = "yes";
        }
    }
}
"#;
    let diags = collect(php);
    assert!(
        has_property_error(&diags),
        "Expected property type error in while loop, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: type juggling (non-strict mode)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_int_assigned_to_string_property_non_strict() {
    // PHP coerces int to string in non-strict mode.
    let php = r#"<?php
class Label {
    public string $text;

    public function set(): void {
        $this->text = 42;
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag int assigned to string property (type juggling), got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_float_assigned_to_string_property_non_strict() {
    let php = r#"<?php
class Config {
    public string $value;

    public function set(): void {
        $this->value = 3.14;
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag float assigned to string property (type juggling), got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_int_assigned_to_float_property() {
    // int is always widened to float in PHP.
    let php = r#"<?php
class Measurement {
    public float $value;

    public function set(): void {
        $this->value = 42;
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag int assigned to float property (widening), got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: strict_types interactions
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn strict_types_flags_int_assigned_to_string_property() {
    let php = r#"<?php
declare(strict_types=1);

class Config {
    public string $name;

    public function set(): void {
        $this->name = 42;
    }
}
"#;
    let diags = collect(php);
    assert!(
        has_property_error(&diags),
        "Expected error for int assigned to string property under strict_types=1, got: {diags:?}"
    );
}

#[test]
fn strict_types_still_allows_null_for_nullable_property() {
    let php = r#"<?php
declare(strict_types=1);

class Config {
    public ?string $name;

    public function set(): void {
        $this->name = null;
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "strict_types should not affect nullable null assignment, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: class hierarchy (subclass / interface)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_subclass_assigned_to_parent_property() {
    let php = r#"<?php
class Animal {}
class Cat extends Animal {}

class Zoo {
    public Animal $animal;

    public function set(): void {
        $this->animal = new Cat();
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag subclass Cat assigned to Animal property, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_interface_implementor_assigned_to_interface_property() {
    let php = r#"<?php
interface Loggable {
    public function log(): void;
}
class FileLogger implements Loggable {
    public function log(): void {}
}

class App {
    public Loggable $logger;

    public function init(): void {
        $this->logger = new FileLogger();
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag interface implementor assigned to interface property, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_deep_inheritance_assigned_to_base_property() {
    let php = r#"<?php
class Base {}
class Middle extends Base {}
class Leaf extends Middle {}

class Container {
    public Base $item;

    public function set(): void {
        $this->item = new Leaf();
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag deep subclass assigned to base-typed property, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_object_property_with_class_instance() {
    let php = r#"<?php
class Foo {}

class Container {
    public object $item;

    public function set(): void {
        $this->item = new Foo();
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag class instance assigned to object property, got: {diags:?}"
    );
}

#[test]
fn flags_wrong_class_assigned_to_typed_property() {
    let php = r#"<?php
class Dog {}
class Cat {}

class Kennel {
    public Dog $pet;

    public function adopt(): void {
        $this->pet = new Cat();
    }
}
"#;
    let diags = collect(php);
    assert!(
        has_property_error(&diags),
        "Expected error for Cat assigned to Dog property, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: Stringable objects assigned to string property
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_stringable_assigned_to_string_property() {
    let php = r#"<?php
class HtmlString {
    public function __toString(): string { return ''; }
}

class Page {
    public string $content;

    public function set(): void {
        $this->content = new HtmlString();
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag Stringable object assigned to string property, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: static property assignments
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_correct_static_property_self() {
    let php = r#"<?php
class Registry {
    public static int $count = 0;

    public static function increment(): void {
        self::$count = 1;
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag correct int assigned to int static property via self::, got: {diags:?}"
    );
}

#[test]
fn does_not_yet_flag_wrong_type_to_static_property_self() {
    // BUG: static property assignments via self:: are not type-checked
    // against the declared type. This test documents the current
    // (incorrect) behavior. When fixed, flip the assertion.
    let php = r#"<?php
class Registry {
    public static int $count = 0;

    public static function reset(): void {
        self::$count = "wrong";
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Known gap: static property assignment via self:: not yet type-checked, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_correct_static_property_static() {
    let php = r#"<?php
class Cache {
    public static string $key = '';

    public static function setKey(): void {
        static::$key = "new_key";
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag correct string assigned via static::, got: {diags:?}"
    );
}

#[test]
fn does_not_yet_flag_wrong_type_to_static_property_static() {
    // BUG: static property assignments via static:: are not type-checked
    // against the declared type. This test documents the current
    // (incorrect) behavior. When fixed, flip the assertion.
    let php = r#"<?php
class Cache {
    public static string $key = '';

    public static function setKey(): void {
        static::$key = [];
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Known gap: static property assignment via static:: not yet type-checked, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: nullable property assignments
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_string_assigned_to_nullable_string_property() {
    let php = r#"<?php
class User {
    public ?string $email;

    public function set(): void {
        $this->email = "test@example.com";
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag string assigned to ?string property, got: {diags:?}"
    );
}

#[test]
fn flags_array_assigned_to_nullable_string_property() {
    let php = r#"<?php
class User {
    public ?string $email;

    public function set(): void {
        $this->email = [];
    }
}
"#;
    let diags = collect(php);
    assert!(
        has_property_error(&diags),
        "Expected error for array assigned to ?string property, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: union property types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_string_assigned_to_union_string_int() {
    let php = r#"<?php
class Config {
    public string|int $value;

    public function set(): void {
        $this->value = "hello";
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag string assigned to string|int property, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_null_assigned_to_union_with_null() {
    let php = r#"<?php
class Config {
    public string|int|null $value;

    public function set(): void {
        $this->value = null;
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag null assigned to string|int|null property, got: {diags:?}"
    );
}

#[test]
fn flags_bool_assigned_to_union_string_int_null() {
    let php = r#"<?php
class Config {
    public string|int|null $value;

    public function set(): void {
        $this->value = true;
    }
}
"#;
    let diags = collect(php);
    assert!(
        has_property_error(&diags),
        "Expected error for bool assigned to string|int|null property, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: iterable / callable / array property types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_array_assigned_to_iterable_property() {
    let php = r#"<?php
class DataSource {
    public iterable $items;

    public function set(): void {
        $this->items = [1, 2, 3];
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag array assigned to iterable property, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_array_literal_assigned_to_array_property() {
    let php = r#"<?php
class Container {
    public array $items;

    public function set(): void {
        $this->items = [1, 2, 3];
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag array literal assigned to array property, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: trait properties
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_correct_trait_property_assignment() {
    let php = r#"<?php
trait HasName {
    public string $name;

    public function setName(): void {
        $this->name = "hello";
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag correct assignment in trait method, got: {diags:?}"
    );
}

#[test]
fn flags_wrong_trait_property_assignment() {
    let php = r#"<?php
trait HasName {
    public string $name;

    public function setName(): void {
        $this->name = [];
    }
}
"#;
    let diags = collect(php);
    assert!(
        has_property_error(&diags),
        "Expected error for array assigned to string trait property, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: constructor promoted properties
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_correct_promoted_property_assignment() {
    let php = r#"<?php
class User {
    public function __construct(
        public string $name,
        public int $age,
    ) {}

    public function rename(string $newName): void {
        $this->name = $newName;
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag correct assignment to promoted property, got: {diags:?}"
    );
}

#[test]
fn flags_wrong_promoted_property_assignment() {
    let php = r#"<?php
class User {
    public function __construct(
        public string $name,
        public int $age,
    ) {}

    public function setAge(): void {
        $this->age = "not a number";
    }
}
"#;
    let diags = collect(php);
    assert!(
        has_property_error(&diags),
        "Expected error for string assigned to promoted int property, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: readonly properties
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_correct_readonly_property_assignment() {
    let php = r#"<?php
class Config {
    public readonly string $dsn;

    public function __construct(string $dsn) {
        $this->dsn = $dsn;
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag correct assignment to readonly property, got: {diags:?}"
    );
}

#[test]
fn flags_wrong_readonly_property_assignment() {
    let php = r#"<?php
class Config {
    public readonly string $dsn;

    public function __construct() {
        $this->dsn = [];
    }
}
"#;
    let diags = collect(php);
    assert!(
        has_property_error(&diags),
        "Expected error for array assigned to readonly string property, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: bool / true / false property types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_true_assigned_to_bool_property() {
    let php = r#"<?php
class Flag {
    public bool $active;

    public function enable(): void {
        $this->active = true;
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag true assigned to bool property, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_false_assigned_to_bool_property() {
    let php = r#"<?php
class Flag {
    public bool $active;

    public function disable(): void {
        $this->active = false;
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag false assigned to bool property, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: namespaced classes
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_correct_namespaced_property_assignment() {
    let php = r#"<?php
namespace App\Models;

class User {
    public string $name;

    public function setName(): void {
        $this->name = "Alice";
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag correct assignment in namespaced class, got: {diags:?}"
    );
}

#[test]
fn flags_wrong_namespaced_property_assignment() {
    let php = r#"<?php
namespace App\Models;

class User {
    public string $name;

    public function setName(): void {
        $this->name = [];
    }
}
"#;
    let diags = collect(php);
    assert!(
        has_property_error(&diags),
        "Expected error for array assigned to string property in namespaced class, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: multiple classes in same file
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_across_multiple_classes_correct() {
    let php = r#"<?php
class Foo {
    public string $name;

    public function set(): void {
        $this->name = "foo";
    }
}

class Bar {
    public int $count;

    public function set(): void {
        $this->count = 42;
    }
}

class Baz {
    public bool $flag;

    public function set(): void {
        $this->flag = true;
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag any correct assignments across multiple classes, got: {diags:?}"
    );
}

#[test]
fn only_wrong_class_flagged_among_multiple_property() {
    let php = r#"<?php
class Good {
    public string $name;

    public function set(): void {
        $this->name = "good";
    }
}

class Bad {
    public int $count;

    public function set(): void {
        $this->count = "not a number";
    }
}

class AlsoGood {
    public bool $flag;

    public function set(): void {
        $this->flag = true;
    }
}
"#;
    let diags = collect(php);
    let msgs = property_error_messages(&diags);
    assert_eq!(
        msgs.len(),
        1,
        "Expected exactly one property error (in Bad class), got: {msgs:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: multiple properties, only wrong ones flagged
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn flags_only_wrong_properties_in_same_method() {
    let php = r#"<?php
class Form {
    public string $title;
    public int $priority;
    public bool $active;

    public function fill(): void {
        $this->title = "hello";
        $this->priority = "wrong";
        $this->active = false;
    }
}
"#;
    let diags = collect(php);
    let msgs = property_error_messages(&diags);
    assert_eq!(
        msgs.len(),
        1,
        "Expected exactly one property error (priority), got: {msgs:?}"
    );
    assert!(
        msgs[0].contains("priority"),
        "Expected error to mention 'priority', got: {msgs:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: assignment in switch
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_correct_assignment_in_switch() {
    let php = r#"<?php
class Handler {
    public string $result;

    public function handle(int $code): void {
        switch ($code) {
            case 1:
                $this->result = "one";
                break;
            case 2:
                $this->result = "two";
                break;
            default:
                $this->result = "unknown";
        }
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag correct assignments in switch, got: {diags:?}"
    );
}

#[test]
fn flags_wrong_assignment_in_switch_case() {
    let php = r#"<?php
class Handler {
    public string $result;

    public function handle(int $code): void {
        switch ($code) {
            case 1:
                $this->result = "one";
                break;
            default:
                $this->result = [];
        }
    }
}
"#;
    let diags = collect(php);
    let msgs = property_error_messages(&diags);
    assert_eq!(
        msgs.len(),
        1,
        "Expected one property error (default case), got: {msgs:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: assignment in deeply nested control flow
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_correct_deeply_nested_assignment() {
    let php = r#"<?php
class Processor {
    public string $status;

    public function process(int $a, int $b): void {
        if ($a > 0) {
            if ($b > 0) {
                $this->status = "both positive";
            } else {
                $this->status = "a positive";
            }
        } else {
            $this->status = "a non-positive";
        }
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag correct assignments in deeply nested if, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: assignment in for/do-while
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_correct_assignment_in_for() {
    let php = r#"<?php
class Builder {
    public string $result;

    public function build(array $parts): void {
        $this->result = "";
        for ($i = 0; $i < 10; $i++) {
            $this->result = "built";
        }
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag correct assignment in for loop, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_correct_assignment_in_do_while() {
    let php = r#"<?php
class Poller {
    public int $attempts;

    public function poll(): void {
        $this->attempts = 0;
        do {
            $this->attempts = 1;
        } while (false);
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag correct assignment in do-while, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: string concat / arithmetic expressions
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_string_concat_assigned_to_string_property() {
    let php = r#"<?php
class Greeting {
    public string $message;

    public function set(string $name): void {
        $this->message = "Hello, " . $name;
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag string concat assigned to string property, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: assignment in declare block
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_correct_assignment_in_declare_block() {
    let php = r#"<?php
declare(strict_types=1) {
    class Widget {
        public string $name;

        public function set(): void {
            $this->name = "hello";
        }
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag correct assignment inside declare block, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: method return value assigned to property
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_method_return_assigned_to_matching_property() {
    let php = r#"<?php
class Helper {
    public function getText(): string { return "hello"; }
}

class Consumer {
    public string $text;

    public function consume(Helper $h): void {
        $this->text = $h->getText();
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag method return matching property type, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: self/static typed properties
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_self_assigned_to_self_property() {
    let php = r#"<?php
class Node {
    public ?self $next = null;

    public function link(self $other): void {
        $this->next = $other;
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag self assigned to ?self property, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_new_self_assigned_to_self_property() {
    let php = r#"<?php
class LinkedList {
    public ?self $head = null;

    public function init(): void {
        $this->head = new self();
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag new self() assigned to ?self property, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_null_assigned_to_nullable_self_property() {
    let php = r#"<?php
class Node {
    public ?self $next = null;

    public function unlink(): void {
        $this->next = null;
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag null assigned to ?self property, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Genuine errors: various real mismatches that SHOULD be flagged
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn flags_object_assigned_to_int_property() {
    let php = r#"<?php
class Foo {}

class Container {
    public int $count;

    public function set(): void {
        $this->count = new Foo();
    }
}
"#;
    let diags = collect(php);
    assert!(
        has_property_error(&diags),
        "Expected error for object assigned to int property, got: {diags:?}"
    );
}

#[test]
fn flags_null_to_non_nullable_int_property() {
    let php = r#"<?php
class Counter {
    public int $value;

    public function reset(): void {
        $this->value = null;
    }
}
"#;
    let diags = collect(php);
    assert!(
        has_property_error(&diags),
        "Expected error for null assigned to int property, got: {diags:?}"
    );
}

#[test]
fn flags_bool_assigned_to_string_property() {
    let php = r#"<?php
class Label {
    public string $text;

    public function set(): void {
        $this->text = false;
    }
}
"#;
    let diags = collect(php);
    assert!(
        has_property_error(&diags),
        "Expected error for bool assigned to string property, got: {diags:?}"
    );
}

#[test]
fn flags_string_assigned_to_bool_property() {
    let php = r#"<?php
class Toggle {
    public bool $on;

    public function set(): void {
        $this->on = "yes";
    }
}
"#;
    let diags = collect(php);
    assert!(
        has_property_error(&diags),
        "Expected error for string assigned to bool property, got: {diags:?}"
    );
}

#[test]
fn flags_array_assigned_to_int_property() {
    let php = r#"<?php
class Stats {
    public int $total;

    public function set(): void {
        $this->total = [];
    }
}
"#;
    let diags = collect(php);
    assert!(
        has_property_error(&diags),
        "Expected error for array assigned to int property, got: {diags:?}"
    );
}

#[test]
fn flags_array_assigned_to_bool_property() {
    let php = r#"<?php
class Settings {
    public bool $enabled;

    public function set(): void {
        $this->enabled = [];
    }
}
"#;
    let diags = collect(php);
    assert!(
        has_property_error(&diags),
        "Expected error for array assigned to bool property, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Edge case: compound assignment operators should NOT be flagged
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_concat_assign() {
    let php = r#"<?php
class Builder {
    public string $buffer = '';

    public function append(): void {
        $this->buffer .= "more";
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag .= compound assignment, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_minus_assign() {
    let php = r#"<?php
class Counter {
    public int $count = 10;

    public function decrement(): void {
        $this->count -= 1;
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag -= compound assignment, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_multiply_assign() {
    let php = r#"<?php
class Scaler {
    public int $value = 1;

    public function scale(): void {
        $this->value *= 2;
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag *= compound assignment, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_null_coalescing_assign() {
    let php = r#"<?php
class Config {
    public ?string $name = null;

    public function init(): void {
        $this->name ??= "default";
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag ??= compound assignment, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Edge case: dynamic property name — should be skipped
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_dynamic_property_name() {
    let php = r#"<?php
class DynAccess {
    public string $name;
    public int $count;

    public function set(string $prop): void {
        $this->$prop = "anything";
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag dynamic property name assignment, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Edge case: non-$this property access — should be skipped
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_non_this_property_access() {
    // We only check $this->prop and self::$prop, not $other->prop
    let php = r#"<?php
class Foo {
    public int $x;
}

class Bar {
    public function set(Foo $foo): void {
        $foo->x = "wrong but we skip non-$this";
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag assignment to non-$this property (not checked), got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Advanced: nullable class-typed properties
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_class_assigned_to_nullable_class_property() {
    let php = r#"<?php
class User {}

class Session {
    public ?User $currentUser = null;

    public function login(): void {
        $this->currentUser = new User();
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag User assigned to ?User property, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_null_assigned_to_nullable_class_property() {
    let php = r#"<?php
class User {}

class Session {
    public ?User $currentUser;

    public function logout(): void {
        $this->currentUser = null;
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag null assigned to ?User property, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_subclass_assigned_to_nullable_parent_property() {
    let php = r#"<?php
class Vehicle {}
class Car extends Vehicle {}

class Garage {
    public ?Vehicle $parked = null;

    public function park(): void {
        $this->parked = new Car();
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag subclass Car assigned to ?Vehicle property, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Advanced: complex union property types with classes
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_class_in_multi_class_union_property() {
    let php = r#"<?php
class Success {}
class Failure {}
class Pending {}

class ResultHolder {
    public Success|Failure|Pending $result;

    public function set(): void {
        $this->result = new Pending();
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag Pending assigned to Success|Failure|Pending property, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_false_assigned_to_string_or_false_property() {
    let php = r#"<?php
class Cache {
    public string|false $value;

    public function miss(): void {
        $this->value = false;
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag false assigned to string|false property, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_string_assigned_to_string_or_false_property() {
    let php = r#"<?php
class Cache {
    public string|false $value;

    public function hit(): void {
        $this->value = "cached";
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag string assigned to string|false property, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Advanced: intersection typed properties
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_implementing_class_assigned_to_intersection_property() {
    let php = r#"<?php
interface Countable {
    public function count(): int;
}
interface Serializable {
    public function serialize(): string;
}
class SmartCollection implements Countable, Serializable {
    public function count(): int { return 0; }
    public function serialize(): string { return ''; }
}

class Wrapper {
    public Countable&Serializable $collection;

    public function set(): void {
        $this->collection = new SmartCollection();
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag class implementing both interfaces for intersection property, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Advanced: property assigned from typed parameter
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_typed_param_assigned_to_matching_property() {
    let php = r#"<?php
class Config {
    public string $dsn;
    public int $timeout;
    public bool $debug;

    public function configure(string $dsn, int $timeout, bool $debug): void {
        $this->dsn = $dsn;
        $this->timeout = $timeout;
        $this->debug = $debug;
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag typed params assigned to matching properties, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Advanced: property assigned from cast expressions
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_int_cast_assigned_to_int_property() {
    let php = r#"<?php
class Parser {
    public int $value;

    public function parse(string $s): void {
        $this->value = (int) $s;
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag (int) cast assigned to int property, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_string_cast_assigned_to_string_property() {
    let php = r#"<?php
class Formatter {
    public string $output;

    public function format(mixed $v): void {
        $this->output = (string) $v;
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag (string) cast assigned to string property, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_array_cast_assigned_to_array_property() {
    let php = r#"<?php
class Converter {
    public array $data;

    public function convert(object $o): void {
        $this->data = (array) $o;
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag (array) cast assigned to array property, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Advanced: real-world patterns — builder, entity, DTO
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_complex_entity_class() {
    let php = r#"<?php
class Order {
    public int $id;
    public string $status;
    public ?string $notes = null;
    public float $total;
    public bool $paid;
    public array $items;

    public function markPaid(): void {
        $this->paid = true;
        $this->status = "paid";
    }

    public function addNote(string $note): void {
        $this->notes = $note;
    }

    public function clearNote(): void {
        $this->notes = null;
    }

    public function setTotal(float $amount): void {
        $this->total = $amount;
    }

    public function setItems(array $items): void {
        $this->items = $items;
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag any correct assignments in complex entity, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_dto_with_promoted_properties() {
    let php = r#"<?php
class CreateUserRequest {
    public function __construct(
        public string $name,
        public string $email,
        public ?string $phone = null,
        public int $age = 0,
        public bool $active = true,
    ) {}

    public function withPhone(string $phone): void {
        $this->phone = $phone;
    }

    public function withAge(int $age): void {
        $this->age = $age;
    }

    public function deactivate(): void {
        $this->active = false;
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag any correct assignments in DTO with promoted props, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Advanced: assignment from ternary / null coalescing
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_ternary_assigned_to_string_property() {
    let php = r#"<?php
class Formatter {
    public string $mode;

    public function set(bool $flag): void {
        $this->mode = $flag ? "verbose" : "quiet";
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag ternary returning strings assigned to string property, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_null_coalescing_assigned_to_string_property() {
    let php = r#"<?php
class Config {
    public string $name;

    public function set(?string $input): void {
        $this->name = $input ?? "default";
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag null coalescing assigned to string property, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Advanced: private/protected properties
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_correct_private_property_assignment() {
    let php = r#"<?php
class Internal {
    private string $secret;
    protected int $guarded;

    public function init(): void {
        $this->secret = "hidden";
        $this->guarded = 42;
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag correct private/protected property assignments, got: {diags:?}"
    );
}

#[test]
fn flags_wrong_private_property_assignment() {
    let php = r#"<?php
class Internal {
    private string $secret;

    public function init(): void {
        $this->secret = [];
    }
}
"#;
    let diags = collect(php);
    assert!(
        has_property_error(&diags),
        "Expected error for array assigned to private string property, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Advanced: multiple assignment targets in same statement line
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn flags_only_mismatched_in_sequential_assignments() {
    let php = r#"<?php
class MultiProp {
    public string $a;
    public int $b;
    public bool $c;
    public float $d;

    public function setAll(): void {
        $this->a = "ok";
        $this->b = [];
        $this->c = true;
        $this->d = "wrong";
    }
}
"#;
    let diags = collect(php);
    let msgs = property_error_messages(&diags);
    // $b gets array (wrong), $d gets string (wrong in strict; but float
    // may or may not be juggled from string — depends on strict mode).
    // At minimum $b should be flagged.
    assert!(
        msgs.iter().any(|m| m.contains("$b") || m.contains("b")),
        "Expected at least $b to be flagged, got: {msgs:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Advanced: exception hierarchy property
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_exception_subclass_assigned_to_exception_property() {
    let php = r#"<?php
class AppException extends \RuntimeException {}
class ValidationException extends AppException {}

class ErrorHandler {
    public \RuntimeException $lastError;

    public function handle(): void {
        $this->lastError = new ValidationException("bad");
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag deep exception subclass assigned to RuntimeException property, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Advanced: enum property assignment
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_backed_enum_value_in_string_context() {
    // Backed enum ->value is a string, assigning to string property
    // is valid but we don't necessarily track ->value type. Ensure
    // we at least don't false-positive here.
    let php = r#"<?php
enum Color: string {
    case Red = 'red';
    case Blue = 'blue';
}

class Palette {
    public string $primary;

    public function set(): void {
        $this->primary = Color::Red->value;
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag backed enum ->value assigned to string property, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_self_typed_property_assignment() {
    // A property typed `self` (or an array of `self`) must resolve to the
    // enclosing class so an assignment of the concrete type compares equal.
    let php = r#"<?php
namespace App\Models;
class Node {
    private ?self $next = null;

    public function link(): void {
        $this->next = new Node();
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag Node assigned to self-typed property, got: {:?}",
        property_error_messages(&diags)
    );
}

#[test]
fn no_diagnostic_for_intersection_assigned_to_property() {
    // An intersection value `A&B` satisfies each member, so assigning it to
    // a property typed `A` is compatible.
    let php = r#"<?php
interface Countable {}
class Service {}

class Test {
    private Service $service;

    /** @return Service&Countable */
    private function mock() { return null; }

    public function setUp(): void {
        $this->service = $this->mock();
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "Should not flag Service&Countable assigned to Service property, got: {:?}",
        property_error_messages(&diags)
    );
}

// ─── Conditional return type narrows over a broad native union ──────────────

#[test]
fn conditional_return_over_union_assigned_to_array_property_no_error() {
    // Assigning `X::collect([...])` to a `@var array<X>` property should
    // resolve through the method's conditional `@return`, narrowing the
    // literal-array argument to `array<static>` rather than the method's
    // broad native union return type.  Mirrors Spatie LaravelData usage.
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

class Component {
    /** @var array<AccordionData> */
    public array $items;

    public function __construct() {
        $this->items = AccordionData::collect([1, 2, 3]);
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_property_error(&diags),
        "collect([...]) narrowed via conditional should satisfy array<AccordionData> property, got: {}",
        property_error_messages(&diags).join("; ")
    );
}
