use crate::common::{collect_diagnostics_with, create_test_backend, message_text};
use phpantom_lsp::Backend;
use tower_lsp_server::gen_lsp_types::*;

const CODES: &[&str] = &[
    "unused_property",
    "unwritten_property",
    "unused_method",
    "unused_constant",
];

fn diagnostics_on(backend: &Backend, php: &str) -> Vec<Diagnostic> {
    collect_diagnostics_with(backend, php, Backend::collect_slow_diagnostics)
        .into_iter()
        .filter(|d| matches!(&d.code, Some(Code::String(code)) if CODES.contains(&code.as_str())))
        .collect()
}

fn collect(php: &str) -> Vec<String> {
    diagnostics_on(&create_test_backend(), php)
        .iter()
        .map(|d| message_text(&d.message).to_string())
        .collect()
}

// ─── Reported ───────────────────────────────────────────────────────────────

#[test]
fn members_nothing_uses_are_reported() {
    let php = r#"<?php
namespace App;

function line_count(array $lines): int
{
    return 0;
}

final class Invoice
{
    private const LEGACY = 'x';
    private $legacyNumber;
    private ?string $note = null;
    private $lines;

    public function total(): int
    {
        $this->note = 'paid';
        return line_count($this->lines);
    }

    private function recalculate(): void {}
}
"#;
    assert_eq!(
        collect(php),
        vec![
            "Constant App\\Invoice::LEGACY is unused",
            "Property App\\Invoice::$legacyNumber is unused",
            "Property App\\Invoice::$note is never read, only written",
            "Property App\\Invoice::$lines is never written, only read",
            "Method App\\Invoice::recalculate() is unused",
        ]
    );
}

#[test]
fn unused_members_are_dimmed_hints_on_the_name() {
    let php = "<?php\nfinal class A { private function helper(): void {} }\n";
    let diags = diagnostics_on(&create_test_backend(), php);
    assert_eq!(diags.len(), 1);
    let diag = &diags[0];
    assert_eq!(diag.code, Some(Code::String("unused_method".into())));
    assert_eq!(diag.severity, Some(DiagnosticSeverity::Hint));
    assert_eq!(diag.tags, Some(vec![DiagnosticTag::Unnecessary]));
    assert_eq!(diag.range.start, Position::new(1, 33));
    assert_eq!(diag.range.end, Position::new(1, 39));
}

#[test]
fn a_property_only_read_is_not_dimmed() {
    let php = r#"<?php
final class A {
    private $limit;
    public function limit(): int { return $this->limit; }
}
"#;
    let diags = diagnostics_on(&create_test_backend(), php);
    assert_eq!(diags.len(), 1);
    assert_eq!(
        diags[0].code,
        Some(Code::String("unwritten_property".into()))
    );
    assert_eq!(diags[0].tags, None);
}

#[test]
fn a_promoted_property_never_read_is_only_written() {
    let php = r#"<?php
final class A {
    public function __construct(private int $id, private int $used) {}
    public function used(): int { return $this->used; }
}
"#;
    assert_eq!(
        collect(php),
        vec!["Property A::$id is never read, only written"]
    );
}

#[test]
fn an_offset_write_only_writes_an_array() {
    let php = r#"<?php
final class A {
    private array $log = [];
    private \ArrayObject $bag;
    public function __construct() { $this->bag = new \ArrayObject(); }
    public function add(string $line): void {
        $this->log[] = $line;
        $this->bag['k'] = $line;
    }
}
"#;
    assert_eq!(
        collect(php),
        vec!["Property A::$log is never read, only written"]
    );
}

#[test]
fn members_of_an_enum_and_an_anonymous_class_are_reported() {
    let php = r#"<?php
enum Suit {
    case Hearts;
    private const OLD = 1;
}
$x = new class {
    private function helper(): void {}
};
"#;
    assert_eq!(
        collect(php),
        vec!["Constant Suit::OLD is unused", "Method helper() is unused"]
    );
}

// ─── Uses ───────────────────────────────────────────────────────────────────

#[test]
fn uses_through_any_receiver_count() {
    let php = r#"<?php
final class Money {
    private const SCALE = 2;
    private static int $count = 0;
    private int $cents;
    public function __construct(int $cents) { $this->cents = $cents; self::$count++; }
    public function equals(Money $other): bool { return $other->cents === $this->cents; }
    public static function make(): static { return new static(static::scale()); }
    private static function scale(): int { return self::SCALE + static::$count; }
}
"#;
    assert_eq!(collect(php), Vec::<String>::new());
}

#[test]
fn a_method_named_by_a_string_or_a_first_class_callable_is_used() {
    let php = r#"<?php
final class A {
    public function run(array $xs): array {
        $f = $this->square(...);
        usort($xs, [$this, 'compare']);
        array_map('self::double', $xs);
        return array_map($f, $xs);
    }
    private function square(int $x): int { return $x * $x; }
    private function compare(int $a, int $b): int { return $a <=> $b; }
    private static function double(int $x): int { return 2 * $x; }
}
"#;
    assert_eq!(collect(php), Vec::<String>::new());
}

#[test]
fn a_dynamic_access_may_use_any_member_of_its_kind() {
    let php = r#"<?php
final class A {
    private int $a = 1;
    private const B = 2;
    private function c(): void {}
    public function get(string $name): mixed { return $this->$name; }
    public function call(string $name): void { $this->{$name}(); }
    public function constant(string $name): mixed { return self::{$name}; }
}
"#;
    assert_eq!(collect(php), Vec::<String>::new());
}

#[test]
fn a_computed_callable_array_may_call_any_method() {
    let php = r#"<?php
final class A {
    public function dispatch(string $event): void { call_user_func([$this, 'on' . $event]); }
    private function onSaved(): void {}
}
"#;
    assert_eq!(collect(php), Vec::<String>::new());
}

#[test]
fn this_handed_to_a_call_may_read_and_write_any_property() {
    let php = r#"<?php
final class A {
    private int $a = 1;
    public function toArray(): array { return get_object_vars($this); }
}
"#;
    assert_eq!(collect(php), Vec::<String>::new());
}

#[test]
fn a_property_passed_by_reference_is_written() {
    let php = r#"<?php
function by_value(array $xs): void {}
function by_reference(array &$xs): void {}
function variadic_by_reference(&...$xs): void {}
final class A {
    private $read;
    private array $passed = [3, 1, 2];
    private array $named = [];
    private array $spread = [];
    private array $unknown = [];
    private array $method = [];
    public function run(\Closure $f): array {
        by_value($this->read);
        by_reference($this->passed);
        by_reference(xs: $this->named);
        variadic_by_reference(1, $this->spread);
        not_declared_anywhere($this->unknown);
        $f($this->method);
        return [$this->passed, $this->named, $this->spread, $this->unknown, $this->method];
    }
}
"#;
    assert_eq!(
        collect(php),
        vec!["Property A::$read is never written, only read"]
    );
}

#[test]
fn a_property_bound_by_reference_is_read_through_the_binding() {
    let php = r#"<?php
function track(array &$calls): object {
    return new class($calls) {
        private array $calls;
        public function __construct(array &$calls) { $this->calls = &$calls; }
        public function record(string $call): void { $this->calls[] = $call; }
    };
}
function promoted(array &$calls): object {
    return new class($calls) {
        public function __construct(private array &$calls) {}
        public function record(string $call): void { $this->calls[] = $call; }
    };
}
"#;
    assert_eq!(collect(php), Vec::<String>::new());
}

#[test]
fn magic_methods_are_left_alone() {
    let php = r#"<?php
final class Singleton {
    private function __construct() {}
    private function __clone() {}
}
"#;
    assert_eq!(collect(php), Vec::<String>::new());
}

#[test]
fn members_a_framework_manages_are_left_alone() {
    let php = r#"<?php
namespace App;

use Doctrine\ORM\Mapping as ORM;
use Symfony\Contracts\Service\Attribute\Required;

final class Product
{
    #[ORM\Id, ORM\Column]
    private int $id;

    /** @ORM\Column(type="string") */
    private $name;

    #[Required]
    private function setLogger(): void {}

    #[\Deprecated]
    private function legacy(): void {}
}
"#;
    assert_eq!(
        collect(php),
        vec!["Method App\\Product::legacy() is unused"]
    );
}

#[test]
fn a_hooked_property_is_only_reported_when_unused() {
    let php = r#"<?php
final class A {
    private string $first = '';
    private string $full { get => $this->first; }
    private string $unused { get => 'x'; }
    public function full(): string { return $this->full; }
    public function setFirst(string $v): void { $this->first = $v; }
}
"#;
    assert_eq!(collect(php), vec!["Property A::$unused is unused"]);
}

#[test]
fn a_default_counts_as_a_write() {
    let php = r#"<?php
final class A {
    private int $limit = 10;
    private static int $count;
    public function limit(): int { return $this->limit + self::$count; }
}
"#;
    assert_eq!(
        collect(php),
        vec!["Property A::$count is never written, only read"]
    );
}

#[test]
fn a_typed_property_nothing_writes_is_left_to_uninitialized_property() {
    let php = r#"<?php
final class A {
    private string $name;
    public function name(): string { return $this->name; }
}
"#;
    assert_eq!(collect(php), Vec::<String>::new());
}

#[test]
fn a_trait_the_class_uses_can_use_its_members() {
    let php = r#"<?php
trait Greets {
    public function greet(): string { return $this->greeting() . $this->name; }
}
final class A {
    use Greets;
    private string $name = 'x';
    private function greeting(): string { return 'Hi '; }
    private function unused(): void {}
}
"#;
    assert_eq!(collect(php), vec!["Method A::unused() is unused"]);
}

#[test]
fn a_trait_accessing_members_dynamically_may_use_any_of_them() {
    let php = r#"<?php
trait Magic {
    public function __get(string $name): mixed { return $this->$name; }
}
final class A {
    use Magic;
    private int $hidden = 1;
    private function unused(): void {}
}
"#;
    assert_eq!(collect(php), Vec::<String>::new());
}

#[test]
fn a_trait_that_cannot_be_read_leaves_the_class_unjudged() {
    let php = r#"<?php
final class A {
    use MissingTrait;
    private function unused(): void {}
}
"#;
    assert_eq!(collect(php), Vec::<String>::new());
}

#[test]
fn private_members_of_a_trait_are_not_judged() {
    let php = r#"<?php
trait T {
    private function helper(): void {}
}
"#;
    assert_eq!(collect(php), Vec::<String>::new());
}

#[test]
fn a_nested_anonymous_class_using_the_member_counts() {
    let php = r#"<?php
final class A {
    private const X = 1;
    public function make(): object {
        return new class { public function x(): int { return A::X; } };
    }
}
"#;
    assert_eq!(collect(php), Vec::<String>::new());
}
