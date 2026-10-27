use crate::common::message_text;
use crate::common::{collect_diagnostics_with, create_test_backend};
use phpantom_lsp::Backend;
use phpantom_lsp::types::PhpVersion;
use tower_lsp_server::gen_lsp_types::*;

// ─── Helpers ────────────────────────────────────────────────────────────────

fn collect(php: &str) -> Vec<Diagnostic> {
    let mut out = collect_diagnostics_with(
        &create_test_backend(),
        php,
        Backend::collect_slow_diagnostics,
    );
    out.retain(|d| {
        d.code
            .as_ref()
            .is_some_and(|c| matches!(c, Code::String(s) if s == "invalid_readonly_write"))
    });
    out
}

fn messages(php: &str) -> Vec<String> {
    collect(php)
        .into_iter()
        .map(|d| message_text(&d.message).to_string())
        .collect()
}

// ─── Writes from outside the declaring class ────────────────────────────────

#[test]
fn a_readonly_property_written_from_top_level_code_is_flagged() {
    let php = r#"<?php
final class Box {
    public function __construct(public readonly int $value) {}
}

$box = new Box(1);
$box->value = 2;
"#;
    let msgs = messages(php);
    assert_eq!(msgs.len(), 1, "expected one diagnostic, got {msgs:?}");
    assert!(
        msgs[0].contains("Box::$value") && msgs[0].contains("outside its declaring class"),
        "unexpected message: {}",
        msgs[0]
    );
}

#[test]
fn a_declared_readonly_property_written_from_another_class_is_flagged() {
    let php = r#"<?php
class Box {
    public readonly int $value;

    public function __construct(int $value)
    {
        $this->value = $value;
    }
}

class Mutator {
    public function change(Box $box): void
    {
        $box->value = 2;
    }
}
"#;
    let msgs = messages(php);
    assert_eq!(msgs.len(), 1, "expected one diagnostic, got {msgs:?}");
    assert!(msgs[0].contains("Box::$value"), "message: {}", msgs[0]);
}

fn messages_on_php(php: &str, version: PhpVersion) -> Vec<String> {
    let backend = create_test_backend();
    backend.set_php_version(version);
    collect_diagnostics_with(&backend, php, Backend::collect_slow_diagnostics)
        .into_iter()
        .filter(|d| matches!(&d.code, Some(Code::String(s)) if s == "invalid_readonly_write"))
        .map(|d| message_text(&d.message).to_string())
        .collect()
}

#[test]
fn a_subclass_initializing_its_parents_readonly_property_is_flagged_before_php_8_4() {
    let php = r#"<?php
class Base {
    public readonly int $value;
}

class Child extends Base {
    public function __construct()
    {
        $this->value = 1;
    }
}
"#;
    let msgs = messages_on_php(php, PhpVersion::new(8, 3));
    assert_eq!(msgs.len(), 1, "expected one diagnostic, got {msgs:?}");
    assert!(msgs[0].contains("Base::$value"), "message: {}", msgs[0]);

    // PHP 8.4 made a readonly property `protected(set)`.
    assert!(messages_on_php(php, PhpVersion::new(8, 4)).is_empty());
}

#[test]
fn a_compound_assignment_and_an_increment_are_both_flagged() {
    let php = r#"<?php
final class Box {
    public function __construct(public readonly int $value) {}
}

$box = new Box(1);
$box->value += 2;
$box->value++;
--$box->value;
"#;
    assert_eq!(collect(php).len(), 3);
}

// ─── Writes inside the declaring class ──────────────────────────────────────

#[test]
fn the_constructor_may_initialize_its_own_readonly_properties() {
    let php = r#"<?php
final class Box {
    public readonly int $value;
    public readonly string $label;

    public function __construct(int $value)
    {
        $this->value = $value;
        $this->label = 'box';
    }
}
"#;
    assert!(collect(php).is_empty());
}

#[test]
fn a_write_in_another_method_after_the_constructor_initialized_it_is_flagged() {
    let php = r#"<?php
final class Box {
    public function __construct(public readonly int $value) {}

    public function withValue(int $value): self
    {
        $clone = clone $this;
        $clone->value = $value;

        return $clone;
    }
}
"#;
    let msgs = messages(php);
    assert_eq!(msgs.len(), 1, "expected one diagnostic, got {msgs:?}");
    assert!(
        msgs[0].contains("after the constructor has initialized it"),
        "unexpected message: {}",
        msgs[0]
    );
}

#[test]
fn a_lazily_initialized_readonly_property_is_not_flagged() {
    let php = r#"<?php
final class Box {
    public readonly int $value;

    public function init(int $value): void
    {
        $this->value = $value;
    }
}
"#;
    assert!(collect(php).is_empty());
}

#[test]
fn a_conditional_constructor_assignment_does_not_make_a_later_write_an_error() {
    let php = r#"<?php
final class Box {
    public readonly int $value;

    public function __construct(?int $value)
    {
        if ($value !== null) {
            $this->value = $value;
        }
    }

    public function init(int $value): void
    {
        $this->value = $value;
    }
}
"#;
    assert!(collect(php).is_empty());
}

#[test]
fn clone_may_reinitialize_a_readonly_property() {
    let php = r#"<?php
final class Box {
    public function __construct(public readonly int $value) {}

    public function __clone(): void
    {
        $this->value = 0;
    }
}
"#;
    assert!(collect(php).is_empty());
}

#[test]
fn a_readonly_property_declared_by_a_trait_is_initialized_in_the_using_class() {
    let php = r#"<?php
trait HasValue {
    public readonly int $value;
}

final class Box {
    use HasValue;

    public function __construct(int $value)
    {
        $this->value = $value;
    }
}
"#;
    assert!(collect(php).is_empty());
}

// ─── Receivers other than a plain variable ──────────────────────────────────

#[test]
fn a_receiver_reached_through_a_property_chain_is_resolved() {
    let php = r#"<?php
final class Box {
    public function __construct(public readonly int $value) {}
}

final class Holder {
    public Box $box;

    public function change(): void
    {
        $this->box->value = 2;
    }
}
"#;
    let msgs = messages(php);
    assert_eq!(msgs.len(), 1, "expected one diagnostic, got {msgs:?}");
    assert!(msgs[0].contains("Box::$value"), "message: {}", msgs[0]);
}

#[test]
fn a_receiver_returned_by_a_call_is_resolved() {
    let php = r#"<?php
final class Box {
    public function __construct(public readonly int $value) {}
}

final class Holder {
    public function box(): Box
    {
        return new Box(1);
    }

    public function change(): void
    {
        $this->box()->value = 2;
    }
}
"#;
    assert_eq!(collect(php).len(), 1);
}

#[test]
fn a_union_receiver_with_one_writable_branch_is_not_flagged() {
    let php = r#"<?php
final class Locked {
    public function __construct(public readonly int $value) {}
}

final class Open {
    public int $value = 0;
}

function change(Locked|Open $target): void
{
    $target->value = 2;
}
"#;
    assert!(collect(php).is_empty());
}

// ─── readonly classes ───────────────────────────────────────────────────────

#[test]
fn a_property_of_a_readonly_class_is_readonly_without_the_keyword() {
    let php = r#"<?php
readonly class Point {
    public function __construct(public int $x) {}
}

$point = new Point(1);
$point->x = 2;
"#;
    let msgs = messages(php);
    assert_eq!(msgs.len(), 1, "expected one diagnostic, got {msgs:?}");
    assert!(
        msgs[0].contains("Point::$x") && msgs[0].contains("outside its declaring class"),
        "unexpected message: {}",
        msgs[0]
    );
}

#[test]
fn a_readonly_class_may_still_initialize_its_properties_in_the_constructor() {
    let php = r#"<?php
readonly class Point {
    public int $x;
    public int $y;

    public function __construct(int $x)
    {
        $this->x = $x;
        $this->y = 0;
    }
}
"#;
    assert!(collect(php).is_empty());
}

#[test]
fn a_subclass_of_a_readonly_class_inherits_the_restriction() {
    let php = r#"<?php
readonly class Base {
    public function __construct(public int $value) {}
}

readonly class Child extends Base {}

$child = new Child(1);
$child->value = 2;
"#;
    let msgs = messages(php);
    assert_eq!(msgs.len(), 1, "expected one diagnostic, got {msgs:?}");
    assert!(msgs[0].contains("Base::$value"), "message: {}", msgs[0]);
}

#[test]
fn a_property_declared_by_a_non_readonly_parent_stays_writable() {
    // PHP rejects this hierarchy outright, but until the class keyword is
    // typed the property is the parent's and the parent says nothing about
    // readonly.
    let php = r#"<?php
class Base {
    public int $value = 0;
}

readonly class Child extends Base {}

$child = new Child();
$child->value = 2;
"#;
    assert!(collect(php).is_empty());
}

// ─── unset() ────────────────────────────────────────────────────────────────

#[test]
fn unsetting_a_readonly_property_from_outside_the_class_is_flagged() {
    let php = r#"<?php
final class Box {
    public function __construct(public readonly int $value) {}
}

$box = new Box(1);
unset($box->value);
"#;
    let msgs = messages(php);
    assert_eq!(msgs.len(), 1, "expected one diagnostic, got {msgs:?}");
    assert!(
        msgs[0].contains("Cannot unset readonly property Box::$value"),
        "unexpected message: {}",
        msgs[0]
    );
}

#[test]
fn unsetting_a_readonly_property_the_constructor_initialized_is_flagged() {
    let php = r#"<?php
final class Box {
    public function __construct(public readonly int $value) {}

    public function forget(): void
    {
        unset($this->value);
    }
}
"#;
    let msgs = messages(php);
    assert_eq!(msgs.len(), 1, "expected one diagnostic, got {msgs:?}");
    assert!(
        msgs[0].contains("Cannot unset readonly property")
            && msgs[0].contains("after the constructor has initialized it"),
        "unexpected message: {}",
        msgs[0]
    );
}

#[test]
fn unsetting_a_readonly_property_before_it_is_initialized_is_allowed() {
    let php = r#"<?php
final class Box {
    public readonly int $value;

    public function __construct()
    {
        unset($this->value);
        $this->value = 1;
    }
}
"#;
    assert!(collect(php).is_empty());
}

// ─── Writes through the property's value ────────────────────────────────────

#[test]
fn appending_to_a_readonly_array_property_is_flagged() {
    let php = r#"<?php
final class Box {
    public function __construct(public readonly array $items) {}
}

$box = new Box([]);
$box->items[] = 'x';
"#;
    let msgs = messages(php);
    assert_eq!(msgs.len(), 1, "expected one diagnostic, got {msgs:?}");
    assert!(
        msgs[0].contains("Cannot indirectly modify readonly property Box::$items"),
        "unexpected message: {}",
        msgs[0]
    );
}

#[test]
fn writing_and_unsetting_an_offset_of_a_readonly_array_property_are_flagged() {
    let php = r#"<?php
final class Box {
    public function __construct(public readonly array $items) {}
}

$box = new Box([]);
$box->items['key'] = 'x';
$box->items[0][1] = 'x';
unset($box->items['key']);
"#;
    assert_eq!(collect(php).len(), 3);
}

#[test]
fn an_offset_write_inside_the_declaring_class_is_flagged_too() {
    // Unlike an assignment, an offset write never counts as initializing
    // the property, so even the constructor may not make one.
    let php = r#"<?php
final class Box {
    public readonly array $items;

    public function __construct()
    {
        $this->items[] = 1;
    }

    public function add(): void
    {
        $this->items[] = 2;
    }
}
"#;
    assert_eq!(collect(php).len(), 2);
}

#[test]
fn an_offset_write_on_a_readonly_array_access_property_is_not_flagged() {
    let php = r#"<?php
final class Box {
    public function __construct(public readonly \ArrayObject $items) {}
}

$box = new Box(new \ArrayObject());
$box->items['key'] = 'x';
"#;
    assert!(collect(php).is_empty());
}

#[test]
fn an_offset_write_on_a_writable_array_property_is_not_flagged() {
    let php = r#"<?php
final class Box {
    public array $items = [];
}

$box = new Box();
$box->items[] = 'x';
"#;
    assert!(collect(php).is_empty());
}

#[test]
fn taking_a_reference_to_a_readonly_property_is_flagged() {
    let php = r#"<?php
final class Box {
    public readonly int $value;

    public function __construct()
    {
        $inside = &$this->value;
        $this->value = 1;
    }
}

$box = new Box();
$outside = &$box->value;
"#;
    let msgs = messages(php);
    assert_eq!(msgs.len(), 2, "expected two diagnostics, got {msgs:?}");
    assert!(
        msgs.iter()
            .all(|m| m.contains("Cannot indirectly modify readonly property Box::$value")),
        "unexpected messages: {msgs:?}"
    );
}

// ─── Destructuring and foreach targets ──────────────────────────────────────

#[test]
fn a_readonly_property_in_a_destructuring_pattern_is_flagged() {
    let php = r#"<?php
final class Box {
    public function __construct(public readonly int $value) {}
}

$box = new Box(1);
$pair = [1, 2];
[$box->value, $other] = $pair;
list($box->value) = $pair;
[['nested' => $box->value]] = [$pair];
"#;
    let msgs = messages(php);
    assert_eq!(msgs.len(), 3, "expected three diagnostics, got {msgs:?}");
    assert!(
        msgs.iter()
            .all(|m| m.contains("Cannot modify readonly property Box::$value")),
        "unexpected messages: {msgs:?}"
    );
}

#[test]
fn a_readonly_property_used_as_a_foreach_target_is_flagged() {
    let php = r#"<?php
final class Box {
    public function __construct(public readonly int $value) {}
}

$box = new Box(1);
foreach ([1, 2] as $box->value) {
}
"#;
    let msgs = messages(php);
    assert_eq!(msgs.len(), 1, "expected one diagnostic, got {msgs:?}");
    assert!(
        msgs[0].contains("Cannot modify readonly property Box::$value"),
        "unexpected message: {}",
        msgs[0]
    );
}

// ─── Non-readonly writes ────────────────────────────────────────────────────

#[test]
fn a_writable_property_is_never_flagged() {
    let php = r#"<?php
final class Box {
    public function __construct(public int $value) {}
}

$box = new Box(1);
$box->value = 2;
"#;
    assert!(collect(php).is_empty());
}

#[test]
fn a_readonly_property_read_outside_the_class_is_not_a_write() {
    let php = r#"<?php
final class Box {
    public function __construct(public readonly int $value) {}
}

$box = new Box(1);
$other = $box->value;
"#;
    assert!(collect(php).is_empty());
}

#[test]
fn an_unresolved_receiver_is_never_flagged() {
    let php = r#"<?php
function mystery() {}

$thing = mystery();
$thing->value = 2;
"#;
    assert!(collect(php).is_empty());
}

// ─── Writes inside the constructor after it initialized the property ───────

#[test]
fn a_second_constructor_assignment_is_flagged() {
    let php = r#"<?php
final class Box {
    public readonly int $value;

    public function __construct(int $value)
    {
        $this->value = $value;
        $this->value = 17;
    }
}
"#;
    assert_eq!(
        messages(php),
        vec![
            "Cannot modify readonly property Box::$value after the constructor has already initialized it"
        ]
    );
}

#[test]
fn a_conditional_write_after_the_constructor_initialized_it_is_flagged() {
    let php = r#"<?php
final class Box {
    public readonly int $value;

    public function __construct(int $value, bool $reset)
    {
        $this->value = $value;
        if ($reset) {
            unset($this->value);
        }
    }
}
"#;
    assert_eq!(
        messages(php),
        vec![
            "Cannot unset readonly property Box::$value after the constructor has already initialized it"
        ]
    );
}

#[test]
fn a_conditional_first_assignment_does_not_make_a_later_one_an_error() {
    let php = r#"<?php
final class Box {
    public readonly int $value;

    public function __construct(?int $value)
    {
        if ($value !== null) {
            $this->value = $value;
            return;
        }
        $this->value = 0;
    }
}
"#;
    assert!(collect(php).is_empty());
}

#[test]
fn reading_the_property_in_its_own_assignment_is_not_a_second_write() {
    let php = r#"<?php
final class Box {
    public readonly int $value;
    public readonly int $double;

    public function __construct(int $value)
    {
        $this->value = $value;
        $this->double = $this->value * 2;
    }
}
"#;
    assert!(collect(php).is_empty());
}

// ─── Properties readonly by docblock ────────────────────────────────────────

#[test]
fn a_docblock_readonly_property_written_from_outside_is_flagged() {
    let php = r#"<?php
final class Invoice {
    /** @readonly */
    public int $total;

    public function __construct(int $total)
    {
        $this->total = $total;
    }
}

$invoice = new Invoice(1);
$invoice->total = 0;
"#;
    let msgs = messages(php);
    assert_eq!(
        msgs,
        vec!["Cannot modify @readonly property Invoice::$total from outside its declaring class"]
    );
}

#[test]
fn every_vendor_spelling_of_the_readonly_tag_counts() {
    let php = r#"<?php
final class Invoice {
    /** @psalm-readonly */
    public int $a = 0;
    /** @phpstan-readonly */
    public int $b = 0;
}

function change(Invoice $invoice): void {
    $invoice->a = 1;
    $invoice->b = 1;
}
"#;
    assert_eq!(messages(php).len(), 2, "{:?}", messages(php));
}

#[test]
fn a_second_constructor_assignment_of_a_docblock_readonly_property_is_flagged() {
    let php = r#"<?php
final class Invoice {
    /** @readonly */
    private int $total;

    public function __construct(int $total)
    {
        $this->total = $total;
        $this->total = 17;
    }
}
"#;
    assert_eq!(
        messages(php),
        vec![
            "Cannot modify @readonly property Invoice::$total after the constructor has already initialized it"
        ]
    );
}

#[test]
fn a_docblock_readonly_property_written_after_the_constructor_is_flagged() {
    let php = r#"<?php
final class Invoice {
    /** @readonly */
    private int $total;

    public function __construct(int $total)
    {
        $this->total = $total;
    }

    public function reset(): void
    {
        $this->total = 0;
    }
}
"#;
    let msgs = messages(php);
    assert_eq!(msgs.len(), 1, "{msgs:?}");
    assert!(
        msgs[0].contains("after the constructor has initialized it"),
        "{}",
        msgs[0]
    );
}

#[test]
fn a_subclass_writing_a_docblock_readonly_property_is_flagged_on_php_8_4() {
    let php = r#"<?php
class Base {
    /** @readonly */
    protected int $value;
}

final class Child extends Base {
    public function __construct()
    {
        $this->value = 1;
    }
}
"#;
    let msgs = messages_on_php(php, PhpVersion::new(8, 4));
    assert_eq!(msgs.len(), 1, "{msgs:?}");
    assert!(msgs[0].contains("Base::$value"), "{}", msgs[0]);
}

#[test]
fn filling_a_docblock_readonly_array_in_the_constructor_is_not_flagged() {
    let php = r#"<?php
final class Bag {
    /**
     * @readonly
     * @var list<int>
     */
    private array $items;

    public function __construct()
    {
        $this->items = [];
        $this->items[] = 1;
        $ref = &$this->items;
    }
}
"#;
    assert!(messages(php).is_empty(), "{:?}", messages(php));
}

#[test]
fn an_offset_write_to_a_docblock_readonly_array_outside_is_flagged() {
    let php = r#"<?php
final class Bag {
    /**
     * @readonly
     * @var list<int>
     */
    public array $items = [];
}

function fill(Bag $bag): void {
    $bag->items[] = 1;
}
"#;
    let msgs = messages(php);
    assert_eq!(
        msgs,
        vec!["Cannot modify @readonly property Bag::$items from outside its declaring class"]
    );
}

#[test]
fn private_mutation_leaves_writes_inside_the_class_alone() {
    let php = r#"<?php
final class Counter {
    /**
     * @readonly
     * @psalm-allow-private-mutation
     */
    public int $count = 0;

    /** @psalm-readonly-allow-private-mutation */
    public int $total = 0;

    public function __construct()
    {
        $this->count = 0;
        $this->total = 0;
    }

    public function bump(): void
    {
        $this->count++;
        $this->total += 1;
    }
}

function change(Counter $counter): void {
    $counter->count = 5;
    $counter->total = 5;
}
"#;
    let msgs = messages(php);
    assert_eq!(msgs.len(), 2, "{msgs:?}");
    assert!(
        msgs.iter()
            .all(|m| m.contains("outside its declaring class")),
        "{msgs:?}"
    );
}
