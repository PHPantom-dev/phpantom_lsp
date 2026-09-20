//! The proofs a body carries from where they are written to where they
//! are read.
//!
//! Each case here is a guard or an assignment the source really makes and
//! the walker used to drop on the way to the read: a static property
//! written and then returned, an assignment inside a `try` whose `catch`
//! rethrows, an `&&` chain in a match arm, an identity check against an
//! enum case, and a loop condition that narrows its own operands. Losing
//! any of them surfaces as a `?T` where the source proved `T`, so the
//! null-argument and return-type diagnostics are what these assert on.

use crate::common::create_test_backend;
use tower_lsp::lsp_types::*;

/// Run the slow diagnostic pipeline (which activates the forward
/// walker's scope cache, as a real analysis does) and keep only the
/// diagnostics a lost narrowing produces.
fn type_diagnostics(php: &str) -> Vec<String> {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    backend.update_ast(uri, php);
    let mut out = Vec::new();
    backend.collect_slow_diagnostics(uri, php, &mut out);
    out.iter()
        .filter(|d| {
            d.code.as_ref().is_some_and(|c| {
                matches!(c, NumberOrString::String(s)
                    if s == "type_mismatch_argument" || s == "type_mismatch_return")
            })
        })
        .map(|d| d.message.clone())
        .collect()
}

fn assert_no_type_errors(php: &str) {
    let messages = type_diagnostics(php);
    assert!(
        messages.is_empty(),
        "expected the narrowing to reach the read, got: {messages:?}"
    );
}

fn assert_type_error(php: &str) {
    let messages = type_diagnostics(php);
    assert_eq!(
        messages.len(),
        1,
        "expected exactly one type error, got: {messages:?}"
    );
}

// ─── Static properties ──────────────────────────────────────────────────────

const STATIC_SCAFFOLD: &str = r#"<?php
namespace Repro;

class Repo {}

class Registry {
    private static ?Repo $repo = null;

    public static function takes(Repo $r): void {}
"#;

#[test]
fn the_lazy_init_idiom_leaves_no_null_behind() {
    assert_no_type_errors(&format!(
        r#"{STATIC_SCAFFOLD}
    public static function repo(): Repo
    {{
        if (self::$repo === null) {{
            self::$repo = new Repo();
        }}

        return self::$repo;
    }}
}}
"#
    ));
}

#[test]
fn a_write_to_a_static_property_is_what_the_next_read_sees() {
    assert_no_type_errors(&format!(
        r#"{STATIC_SCAFFOLD}
    public static function go(): void
    {{
        self::$repo = new Repo();
        self::takes(self::$repo);
    }}
}}
"#
    ));
}

#[test]
fn a_guard_that_throws_proves_the_static_property_afterwards() {
    assert_no_type_errors(&format!(
        r#"{STATIC_SCAFFOLD}
    public static function go(): void
    {{
        if (self::$repo === null) {{
            throw new \Exception('unset');
        }}

        self::takes(self::$repo);
    }}
}}
"#
    ));
}

#[test]
fn a_static_property_of_another_class_is_tracked_under_its_own_name() {
    assert_no_type_errors(
        r#"<?php
namespace Repro;

class Repo {}

class Holder {
    public static ?Repo $repo = null;
}

class Registry {
    public static function takes(Repo $r): void {}

    public static function go(): void
    {
        Holder::$repo = new Repo();
        self::takes(Holder::$repo);
    }
}
"#,
    );
}

#[test]
fn a_later_write_replaces_what_an_earlier_one_proved() {
    assert_type_error(&format!(
        r#"{STATIC_SCAFFOLD}
    public static function go(): void
    {{
        self::$repo = new Repo();
        self::$repo = null;
        self::takes(self::$repo);
    }}
}}
"#
    ));
}

#[test]
fn a_write_on_only_one_branch_proves_nothing_after_the_join() {
    assert_type_error(&format!(
        r#"{STATIC_SCAFFOLD}
    public static function go(bool $flag): void
    {{
        if ($flag) {{
            self::$repo = new Repo();
        }}

        self::takes(self::$repo);
    }}
}}
"#
    ));
}

#[test]
fn an_untouched_static_property_keeps_its_declared_type() {
    assert_type_error(
        r#"<?php
namespace Repro;

class Repo {}

class Holder {
    public static ?Repo $repo = null;
}

class Registry {
    public static function takes(Repo $r): void {}

    public static function go(): void
    {
        self::takes(Holder::$repo);
    }
}
"#,
    );
}

// ─── Assignments inside a `try` ─────────────────────────────────────────────

const TRY_SCAFFOLD: &str = r#"<?php
namespace Repro;

class Holder {}

class Runner {
    public function takes(Holder $h): void {}
"#;

#[test]
fn an_assignment_in_a_try_survives_a_catch_that_rethrows() {
    assert_no_type_errors(&format!(
        r#"{TRY_SCAFFOLD}
    public function go(?Holder $h): void
    {{
        if (!$h) {{
            try {{
                $h = new Holder();
            }} catch (\RuntimeException) {{
                throw new \LogicException('x');
            }}
        }}

        $this->takes($h);
    }}
}}
"#
    ));
}

#[test]
fn a_catch_that_falls_through_still_joins_its_state() {
    assert_type_error(&format!(
        r#"{TRY_SCAFFOLD}
    public function go(?Holder $h): void
    {{
        if (!$h) {{
            try {{
                $h = new Holder();
            }} catch (\RuntimeException) {{
                // Swallowed: `$h` is still null on this path.
            }}
        }}

        $this->takes($h);
    }}
}}
"#
    ));
}

// ─── `&&` chains in a match arm ─────────────────────────────────────────────

#[test]
fn an_and_chain_in_a_match_arm_narrows_its_own_operands() {
    assert_no_type_errors(
        r#"<?php
namespace Repro;

class Holder {}

class Runner {
    public ?Holder $a = null;
    public ?Holder $b = null;

    public function same(Holder $h): bool { return true; }

    public function go(int $kind): bool
    {
        return match ($kind) {
            1 => $this->a && $this->b && $this->same($this->a),
            default => true,
        };
    }
}
"#,
    );
}

// ─── Identity against an enum case ──────────────────────────────────────────

const ENUM_SCAFFOLD: &str = r#"<?php
namespace Repro;

enum Land {
    case Be;
    case Nl;
}

class Runner {
    public function takes(Land $land): bool { return true; }
"#;

#[test]
fn identity_with_an_enum_case_rules_out_null_for_the_rest_of_the_chain() {
    assert_no_type_errors(&format!(
        r#"{ENUM_SCAFFOLD}
    public function go(?Land $land): bool
    {{
        return $land === Land::Be && $this->takes($land);
    }}
}}
"#
    ));
}

#[test]
fn a_guard_on_the_negated_identity_proves_the_case_afterwards() {
    assert_no_type_errors(&format!(
        r#"{ENUM_SCAFFOLD}
    public function go(?Land $land): bool
    {{
        if ($land !== Land::Be) {{
            return false;
        }}

        return $this->takes($land);
    }}
}}
"#
    ));
}

#[test]
fn identity_with_a_constant_that_is_null_proves_nothing() {
    assert_type_error(
        r#"<?php
namespace Repro;

class Land {
    const NONE = null;
}

class Runner {
    public function takes(Land $land): bool { return true; }

    public function go(?Land $land): bool
    {
        return $land === Land::NONE && $this->takes($land);
    }
}
"#,
    );
}

// ─── Loop conditions ────────────────────────────────────────────────────────

const LOOP_SCAFFOLD: &str = r#"<?php
namespace Repro;

class Node {}

class Parser {
    public function parseOptional(): ?Node { return null; }

    public function addChild(array $list, Node $node): bool { return true; }
"#;

#[test]
fn a_do_while_condition_narrows_its_own_operands() {
    assert_no_type_errors(&format!(
        r#"{LOOP_SCAFFOLD}
    public function go(array $list): void
    {{
        do {{
            $node = $this->parseOptional();
        }} while ($node && $this->addChild($list, $node));
    }}
}}
"#
    ));
}

#[test]
fn a_for_condition_narrows_its_own_operands() {
    assert_no_type_errors(&format!(
        r#"{LOOP_SCAFFOLD}
    public function go(array $list): void
    {{
        for ($node = $this->parseOptional(); $node && $this->addChild($list, $node); ) {{
        }}
    }}
}}
"#
    ));
}

// ─── Unions with one class member ───────────────────────────────────────────

const MIXED_UNION_SCAFFOLD: &str = r#"<?php
namespace Repro;

class Decimal {
    public function format(): string { return ''; }
}

class Image {
    public function __construct(string $src) {}
    public function url(): string { return ''; }
}

function takesFloat(float $value): string { return (string) $value; }
"#;

#[test]
fn an_instanceof_else_keeps_the_scalar_half_of_the_union() {
    assert_no_type_errors(&format!(
        r#"{MIXED_UNION_SCAFFOLD}
function clip(Decimal|float $value): string
{{
    if ($value instanceof Decimal) {{
        return $value->format();
    }}
    return takesFloat($value);
}}
"#
    ));
}

#[test]
fn a_negated_instanceof_body_keeps_the_scalar_half_of_the_union() {
    assert_no_type_errors(&format!(
        r#"{MIXED_UNION_SCAFFOLD}
function imgix(Image|string $imgix): string
{{
    if (!$imgix instanceof Image) {{
        $imgix = new Image($imgix);
    }}
    return $imgix->url();
}}
"#
    ));
}

// ─── Repair inside the branch that detects the bad value ────────────────────

const REPAIR_SCAFFOLD: &str = r#"<?php
namespace Repro;

enum Status {
    case Active;
}

function takesString(string $s): string { return $s; }
function takesStatus(Status $s): void {}
"#;

#[test]
fn a_falsy_guard_that_repairs_the_value_merges_without_the_falsy_half() {
    assert_no_type_errors(&format!(
        r#"{REPAIR_SCAFFOLD}
function normalize(): string
{{
    $value = mb_strrchr('x', '\\');
    if (!$value) {{
        $value = 'fallback';
    }}
    return $value;
}}
"#
    ));
}

#[test]
fn an_is_array_guard_that_wraps_the_value_merges_to_an_array() {
    assert_no_type_errors(&format!(
        r#"{REPAIR_SCAFFOLD}
/** @param array<Status>|Status $status */
function toArray(array|Status $status): void
{{
    if (!is_array($status)) {{
        $status = [$status];
    }}
    foreach ($status as $s) {{
        takesStatus($s);
    }}
}}
"#
    ));
}

// ─── Reads that used to go back to the declaration ──────────────────────────
//
// A guard narrows the variable, then something *derived* from it is read:
// an array-dimension fetch, an array literal built around it, or an
// argument handed to one of the array functions whose result type follows
// its input. Each of those has its own resolution path, and each used to
// answer from the `@param`/`@var` the declaration states rather than from
// the scope the guard wrote.

const DERIVED_READ_SCAFFOLD: &str = r#"<?php
namespace Repro;

class Cache {
    public static function get(string $key): mixed { return null; }
}

function takesArgs(array $args): void {}
"#;

#[test]
fn an_is_array_guard_reaches_a_dimension_fetch() {
    assert_no_type_errors(&format!(
        r#"{DERIVED_READ_SCAFFOLD}
class Violation
{{
    /** @var array<int, string> */
    public array $args;

    /** @param array{{args: array<int, string>, message: string}}|string $violationMessage */
    public function __construct(array|string $violationMessage)
    {{
        if (is_array($violationMessage)) {{
            $this->args = $violationMessage['args'];
        }}
    }}
}}
"#
    ));
}

#[test]
fn a_not_null_guard_reaches_an_array_function_on_an_annotated_variable() {
    assert_no_type_errors(&format!(
        r#"{DERIVED_READ_SCAFFOLD}
class Versions
{{
    /** @return list<array{{version: string}}> */
    public function recent(int $limit): array
    {{
        /** @var null|list<array{{version: string}}> $cached */
        $cached = Cache::get('versions');
        if ($cached !== null) {{
            return array_slice($cached, 0, $limit);
        }}
        return [];
    }}
}}
"#
    ));
}

#[test]
fn a_phpstan_assert_reaches_a_dimension_fetch() {
    assert_no_type_errors(&format!(
        r#"{DERIVED_READ_SCAFFOLD}
class Asserts
{{
    /** @phpstan-assert !null $actual */
    public static function assertNotNull(mixed $actual): void {{}}
}}

class SectionTest extends Asserts
{{
    public function testSection(): void
    {{
        /** @var null|array{{categories: list<array{{id: int}}>}} $section */
        $section = Cache::get('section');
        self::assertNotNull($section);
        takesArgs($section['categories']);
    }}
}}
"#
    ));
}

// ─── A checked call is the same call where it is written again ──────────────

const REPEATED_CALL_SCAFFOLD: &str = r#"<?php
namespace Repro;

class User {}

class Session
{
    public static function current(): ?User { return null; }
}

function currentUser(): ?User { return null; }
function render(User $user): void {}
"#;

/// `if (currentUser())` proves the call's result non-null, and writing the
/// same call again inside the branch is the idiom the check exists for.
#[test]
fn a_guard_on_a_plain_function_call_narrows_the_repeated_call() {
    assert_no_type_errors(&format!(
        r#"{REPEATED_CALL_SCAFFOLD}
function show(): void
{{
    if (currentUser()) {{
        render(currentUser());
    }}
}}
"#
    ));
}

/// The same for a static call, whose key names the class rather than a
/// receiver variable.
#[test]
fn a_guard_on_a_static_call_narrows_the_repeated_call() {
    assert_no_type_errors(&format!(
        r#"{REPEATED_CALL_SCAFFOLD}
function show(): void
{{
    if (Session::current() !== null) {{
        render(Session::current());
    }}
}}
"#
    ));
}

/// Negative control: the guard says nothing about a *different* call, so
/// the nullable return survives.
#[test]
fn a_guard_on_one_call_leaves_another_alone() {
    assert_type_error(&format!(
        r#"{REPEATED_CALL_SCAFFOLD}
function show(): void
{{
    if (currentUser()) {{
        render(Session::current());
    }}
}}
"#
    ));
}

/// A ternary condition proves the same thing an `if` condition does, and
/// its arms are where the repeated call is written: the
/// "re-check-and-reuse" idiom for a function that signals failure with a
/// value rather than an exception.
#[test]
fn a_ternary_condition_narrows_the_call_repeated_in_its_own_arm() {
    assert_no_type_errors(&format!(
        r#"{REPEATED_CALL_SCAFFOLD}
function show(): void
{{
    $user = currentUser() !== null ? currentUser() : null;
    if ($user !== null) {{
        render($user);
    }}
}}
"#
    ));
}

/// The else arm carries the inverse proof, so the negated spelling of the
/// same idiom narrows there instead.
#[test]
fn a_negated_ternary_condition_narrows_the_call_in_its_else_arm() {
    assert_no_type_errors(&format!(
        r#"{REPEATED_CALL_SCAFFOLD}
function show(User $fallback): void
{{
    render(currentUser() === null ? $fallback : currentUser());
}}
"#
    ));
}

/// Negative control for the ternary form: a check on one call leaves a
/// different call in the arm alone.
#[test]
fn a_ternary_condition_on_one_call_leaves_another_alone() {
    assert_type_error(&format!(
        r#"{REPEATED_CALL_SCAFFOLD}
function show(User $fallback): void
{{
    render(currentUser() !== null ? Session::current() : $fallback);
}}
"#
    ));
}

// ─── A `continue` guard reaches the copy the loop body makes of it ──────────

const CONTINUE_GUARD_SCAFFOLD: &str = r#"<?php
namespace Repro;

class Country {}

class Sheet
{
    /** @return array{0: false|string, 1: Country|false} */
    private function columnValues(string $key): array { return [false, false]; }

    private function updatePrice(int $productId, Country $market, array $row): void {}
"#;

/// `continue` on `!$newMarket` rules `false` out for the rest of the
/// iteration, so the copy two lines down only ever stores a `Country` —
/// including on the path where the copy is itself guarded and the merged
/// state is what the call reads.
#[test]
fn a_continue_guard_reaches_a_variable_the_value_is_copied_into() {
    assert_no_type_errors(&format!(
        r#"{CONTINUE_GUARD_SCAFFOLD}
    /** @param list<string> $keys */
    public function run(array $keys, array $row, int $productId): void
    {{
        $market = null;
        foreach ($keys as $key) {{
            [$dbCol, $newMarket] = $this->columnValues($key);
            if (!$dbCol || !$newMarket) {{
                continue;
            }}
            if (!$market) {{
                $market = $newMarket;
            }}
            $this->updatePrice($productId, $market, $row);
        }}
    }}
}}
"#
    ));
}

/// Negative control: without the guard, the `false` the shape declares is
/// still in play where the copy is read.
#[test]
fn an_unguarded_copy_keeps_the_falsy_union_member() {
    assert_type_error(&format!(
        r#"{CONTINUE_GUARD_SCAFFOLD}
    /** @param list<string> $keys */
    public function run(array $keys, array $row, int $productId): void
    {{
        $market = null;
        foreach ($keys as $key) {{
            [$dbCol, $newMarket] = $this->columnValues($key);
            if (!$market) {{
                $market = $newMarket;
            }}
            $this->updatePrice($productId, $market, $row);
        }}
    }}
}}
"#
    ));
}

// ─── An assertion against a class the subject does not implement ────────────

const MOCK_SCAFFOLD: &str = r#"<?php
namespace Repro;

interface MockObject {}
class MethodNode {}
class FunctionNode {}

class Asserts
{
    /**
     * @template ExpectedType of object
     * @param class-string<ExpectedType> $expected
     * @phpstan-assert =ExpectedType $actual
     */
    public static function assertInstanceOf(string $expected, mixed $actual): void {}
}
"#;

/// A mock really is both the interface it was built as and the class it
/// stands in for, so an assertion naming the class leaves an intersection
/// of the two. Recorded as a union instead, it satisfies neither half of
/// the declared `MethodNode&MockObject`.
#[test]
fn an_assertion_against_an_unrelated_class_intersects_rather_than_unions() {
    assert_no_type_errors(&format!(
        r#"{MOCK_SCAFFOLD}
class Probe extends Asserts
{{
    /** @return MethodNode&MockObject */
    protected function build(MockObject $node)
    {{
        static::assertInstanceOf(MethodNode::class, $node);

        return $node;
    }}
}}
"#
    ));
}

/// The same proof applied to a subject that is *already* an intersection
/// picks the union member it named and leaves the conjunct alone:
/// `(FunctionNode|MethodNode)&MockObject` proven `MethodNode` is
/// `MethodNode&MockObject`.
#[test]
fn an_assertion_picks_one_arm_of_a_union_inside_an_intersection() {
    assert_no_type_errors(&format!(
        r#"{MOCK_SCAFFOLD}
class Probe extends Asserts
{{
    /** @return (FunctionNode|MethodNode)&MockObject */
    private function make(string $class) {{ }}

    /** @return MethodNode&MockObject */
    protected function methodMock()
    {{
        $node = $this->make(MethodNode::class);
        static::assertInstanceOf(MethodNode::class, $node);

        return $node;
    }}

    /** @return FunctionNode&MockObject */
    protected function functionMock()
    {{
        $node = $this->make(FunctionNode::class);
        static::assertInstanceOf(FunctionNode::class, $node);

        return $node;
    }}
}}
"#
    ));
}

/// Negative control: the arm the assertion did *not* name is gone, so
/// returning the narrowed value as the other half is still a mismatch.
#[test]
fn an_assertion_rules_out_the_union_arm_it_did_not_name() {
    assert_type_error(&format!(
        r#"{MOCK_SCAFFOLD}
class Probe extends Asserts
{{
    /** @return (FunctionNode|MethodNode)&MockObject */
    private function make(string $class) {{ }}

    /** @return FunctionNode&MockObject */
    protected function methodMock()
    {{
        $node = $this->make(MethodNode::class);
        static::assertInstanceOf(MethodNode::class, $node);

        return $node;
    }}
}}
"#
    ));
}

// ─── A predicate that promises something about its own receiver ─────────────

const PREDICATE_SCAFFOLD: &str = r#"<?php
namespace PHPStan\Analyser;

class ClassReflection {}
class TraitReflection {}

interface Scope
{
    /** @phpstan-assert-if-true !null $this->getTraitReflection() */
    public function isInTrait(): bool;

    public function getTraitReflection(): ?TraitReflection;

    /** @api */
    public function isInClass(): bool;

    public function getClassReflection(): ?ClassReflection;
}

function useTrait(TraitReflection $reflection): void {}
function useClass(ClassReflection $reflection): void {}
"#;

/// `@phpstan-assert-if-true !null $this->getTraitReflection()` names a
/// member of the *receiver*, not a parameter, so the subject it narrows
/// is that member read through the variable the call was written on.
#[test]
fn a_predicate_narrows_the_member_its_tag_names_on_the_receiver() {
    assert_no_type_errors(&format!(
        r#"{PREDICATE_SCAFFOLD}
function f(Scope $scope): void
{{
    if ($scope->isInTrait()) {{
        $reflection = $scope->getTraitReflection();
        useTrait($reflection);
    }}
}}
"#
    ));
}

/// PHPStan annotates `isInTrait()` and leaves the identical `isInClass()`
/// bare, so the pairing is supplied for it. Every PHPStan extension is
/// written against it regardless of the missing tag.
#[test]
fn phpstan_is_in_class_narrows_the_paired_reflection_getter() {
    assert_no_type_errors(&format!(
        r#"{PREDICATE_SCAFFOLD}
function f(Scope $scope): void
{{
    if ($scope->isInClass()) {{
        $reflection = $scope->getClassReflection();
        useClass($reflection);
    }}
}}
"#
    ));
}

/// Negative control: without the guard the getter is as nullable as it
/// declares itself to be.
#[test]
fn an_unguarded_reflection_getter_keeps_its_null() {
    assert_type_error(&format!(
        r#"{PREDICATE_SCAFFOLD}
function f(Scope $scope): void
{{
    $reflection = $scope->getClassReflection();
    useClass($reflection);
}}
"#
    ));
}

/// An unconditional `@phpstan-assert` can name a path through the
/// receiver too, and then the call takes no arguments at all: the
/// promise is entirely about the object it was made on. The subject is
/// the tag's own path with `$this` replaced by the receiver.
#[test]
fn an_assert_tag_narrows_a_property_of_the_receiver() {
    assert_no_type_errors(
        r#"<?php
namespace Repro;

class Reflection
{
    private ?bool $isDeprecated = null;

    public function isDeprecated(): bool
    {
        if ($this->isDeprecated === null) {
            $this->resolveDeprecation();
        }

        return $this->isDeprecated;
    }

    /** @phpstan-assert bool $this->isDeprecated */
    private function resolveDeprecation(): void
    {
        $this->isDeprecated = false;
    }
}
"#,
    );
}

/// The same tag read through a variable rather than `$this`: the
/// receiver the call was written on is what the tag's `$this` stands
/// for, so the narrowing lands on that object's property.
#[test]
fn an_assert_tag_on_a_receiver_property_follows_the_variable_it_was_called_on() {
    assert_no_type_errors(
        r#"<?php
namespace Repro;

class Loader
{
    public ?string $name = null;

    /** @phpstan-assert string $this->name */
    public function load(): void { $this->name = ''; }
}

function takesName(string $name): void {}

function f(Loader $loader): void
{
    $loader->load();
    takesName($loader->name);
}
"#,
    );
}

/// Negative control: without the call the property is as nullable as it
/// declares itself to be.
#[test]
fn a_receiver_property_keeps_its_null_without_the_asserting_call() {
    assert_type_error(
        r#"<?php
namespace Repro;

class Loader
{
    public ?string $name = null;

    /** @phpstan-assert string $this->name */
    public function load(): void { $this->name = ''; }
}

function takesName(string $name): void {}

function f(Loader $loader): void
{
    takesName($loader->name);
}
"#,
    );
}

/// A `!null` promise about a plain parameter narrows the same way — the
/// tag names no class, so it goes through the type guards rather than the
/// `instanceof` machinery.
#[test]
fn an_assert_if_true_not_null_tag_strips_the_null() {
    assert_no_type_errors(
        r#"<?php
namespace Repro;

class Row {}

class Reader
{
    /** @phpstan-assert-if-true !null $row */
    public function isLoaded(?Row $row): bool { return $row !== null; }
}

function takesRow(Row $row): void {}

function f(Reader $reader, ?Row $row): void
{
    if ($reader->isLoaded($row)) {
        takesRow($row);
    }
}
"#,
    );
}

/// Laravel's `filled()` and `blank()`, copied tag for tag from the
/// framework.  Between them they cover both halves of the pair: the tag
/// naming the branch under test narrows it, and the tag naming the other
/// branch must stay out of it.
const LARAVEL_VALUE_HELPERS: &str = r#"<?php
namespace Repro;

/**
 * @phpstan-assert-if-true !=null|'' $value
 *
 * @phpstan-assert-if-false !=numeric|bool $value
 *
 * @param  mixed  $value
 */
function filled($value): bool { return $value !== null && $value !== ''; }

/**
 * @phpstan-assert-if-false !=null|'' $value
 *
 * @phpstan-assert-if-true !=numeric|bool $value
 *
 * @param  mixed  $value
 */
function blank($value): bool { return $value === null || $value === ''; }

function takesString(string $value): void {}
"#;

/// When the asserted type is a union (e.g. `!=null|''` from Laravel's
/// `filled()`), each member must be excluded independently so that at
/// least the `null` guard fires and strips the null from the subject.
#[test]
fn an_assert_if_true_with_union_asserted_type_strips_null() {
    assert_no_type_errors(&format!(
        r#"{LARAVEL_VALUE_HELPERS}
function f(?string $search): void
{{
    if (filled($search)) {{
        takesString($search);
    }}
}}
"#
    ));
}

/// The same promise read from the other side: an early return on the
/// negated call leaves the rest of the body with the narrowed type.
#[test]
fn an_early_return_on_a_negated_assert_call_narrows_the_rest_of_the_body() {
    assert_no_type_errors(&format!(
        r#"{LARAVEL_VALUE_HELPERS}
function f(?string $search): void
{{
    if (! filled($search)) {{
        return;
    }}
    takesString($search);
}}
"#
    ));
}

/// `blank()` carries the same pair with the branches swapped, so the
/// `-if-false` tag is the one that has to narrow here.
#[test]
fn an_assert_if_false_with_union_asserted_type_narrows_the_else_branch() {
    assert_no_type_errors(&format!(
        r#"{LARAVEL_VALUE_HELPERS}
function f(?string $search): void
{{
    if (blank($search)) {{
        return;
    }}
    takesString($search);
}}
"#
    ));
}

/// An `-if-true` / `-if-false` tag written in the equality form (`!=Type`)
/// is a one-way implication: a failed comparison rules nothing out, so the
/// tag must contribute nothing to the branch it does not name.  Laravel's
/// `filled()` promises `!=numeric|bool` only when it returns *false*, and
/// inverting that into the truthy branch typed every filled value as
/// `numeric|bool` — reporting `takesString($search)` as `got bool`.
#[test]
fn an_equality_assertion_does_not_invert_into_the_branch_it_does_not_name() {
    assert_no_type_errors(&format!(
        r#"{LARAVEL_VALUE_HELPERS}
function f(?string $search): void
{{
    if (filled($search)) {{
        takesString($search);
    }}

    if (! blank($search)) {{
        takesString($search);
    }}
}}
"#
    ));
}

/// The subtype form stays invertible, which is what makes the equality
/// carve-out above a carve-out rather than a blanket rule: `!null` promised
/// on true means the value *is* null when the call returns false.
#[test]
fn a_subtype_assertion_still_inverts_into_the_opposite_branch() {
    let messages = type_diagnostics(
        r#"<?php
namespace Repro;

/**
 * @phpstan-assert-if-true !null $value
 */
function isReady(?string $value): bool { return $value !== null; }

function takesString(string $value): void {}

function f(?string $search): void
{
    if (isReady($search)) {
        takesString($search);
    } else {
        takesString($search);
    }
}
"#,
    );
    assert_eq!(
        messages.len(),
        1,
        "the else branch should still know the value is null, got: {messages:?}"
    );
}

/// A union that is asserted *into* the branch stays a union.  Only the
/// negated form splits into its members, since ruling out `A|B` rules out
/// both; narrowing to `A|B` one member at a time would leave the subject
/// as `B` alone, and every `Cat` in the branch would look like a `Dog`.
#[test]
fn an_asserted_union_narrows_to_every_member_not_just_the_last() {
    let messages = type_diagnostics(
        r#"<?php
namespace Repro;

interface Pet {}
class Cat implements Pet {}
class Dog implements Pet {}

/**
 * @phpstan-assert-if-true Cat|Dog $value
 */
function isKnownPet(Pet $value): bool { return true; }

function takesCat(Cat $value): void {}

function f(Pet $pet): void
{
    if (isKnownPet($pet)) {
        takesCat($pet);
    }
}
"#,
    );
    assert_eq!(messages.len(), 1, "got: {messages:?}");
    assert!(
        messages[0].contains("Repro\\Cat|Repro\\Dog"),
        "both members should survive the assertion, got: {messages:?}"
    );
}

// ─── A ternary that repeats its own subject ─────────────────────────────────

const SELF_TERNARY_SCAFFOLD: &str = r#"<?php
namespace Repro;

/**
 * @property null|string $alt
 * @property string $caption
 */
class Article
{
    public ?string $subtitle = null;
    public string $title = '';

    public function __get(string $name): mixed { return null; }
}

function takesString(string $value): void {}
"#;

/// `$a->alt ? $a->alt : $a->caption` proves the path truthy for its own
/// then-arm. The proof is keyed under the whole path, not under the
/// variable it is rooted at, which is why the arm has to look for it
/// there.
#[test]
fn a_self_referencing_ternary_narrows_a_property_path() {
    assert_no_type_errors(&format!(
        r#"{SELF_TERNARY_SCAFFOLD}
function render(Article $article): void
{{
    $alt = $article->alt ? $article->alt : $article->caption;
    takesString($alt);
}}
"#
    ));
}

/// The same for a real declared property, which took the same path and
/// lost the same proof.
#[test]
fn a_self_referencing_ternary_narrows_a_declared_property() {
    assert_no_type_errors(&format!(
        r#"{SELF_TERNARY_SCAFFOLD}
function render(Article $article): void
{{
    $subtitle = $article->subtitle ? $article->subtitle : $article->title;
    takesString($subtitle);
}}
"#
    ));
}

/// A `@var` block annotating the statement is authoritative over the
/// assignment it names and nothing else: the ternary in the same
/// statement still narrows.
#[test]
fn a_preceding_var_docblock_does_not_cancel_the_statements_narrowing() {
    assert_no_type_errors(&format!(
        r#"{SELF_TERNARY_SCAFFOLD}
function render(): void
{{
    /** @var Article $article */
    takesString($article->alt ? $article->alt : $article->caption);
}}
"#
    ));
}

/// Negative control: the else arm gets the opposite proof, so returning
/// the nullable half there is still a mismatch.
#[test]
fn the_else_arm_of_a_self_ternary_keeps_the_falsy_half() {
    assert_type_error(&format!(
        r#"{SELF_TERNARY_SCAFFOLD}
function render(Article $article): void
{{
    $alt = $article->caption ? $article->caption : $article->alt;
    takesString($alt);
}}
"#
    ));
}

// ─── Loops over an iterable that proves it has entries ──────────────────────

const FLOOR_SCAFFOLD: &str = r#"<?php
namespace Repro;

function takesInt(int $v): int { return $v; }
"#;

/// `if (!$qtys) { return; }` proves the array non-empty, so the body runs
/// at least once and the sentinel the loop was seeded with is gone by the
/// time the loop ends.
#[test]
fn a_guard_proving_the_iterable_non_empty_drops_the_pre_loop_sentinel() {
    assert_no_type_errors(&format!(
        r#"{FLOOR_SCAFFOLD}
/** @param array<int, int> $qtys */
function floorStock(array $qtys): int
{{
    if (!$qtys) {{
        return 0;
    }}
    $max = null;
    foreach ($qtys as $qty) {{
        if ($max === null) {{
            $max = $qty;
            continue;
        }}
        $max = min($max, $qty);
    }}
    return takesInt($max);
}}
"#
    ));
}

/// The same proof written into the parameter's own type rather than
/// carried in by a guard.
#[test]
fn a_non_empty_array_parameter_drops_the_pre_loop_sentinel() {
    assert_no_type_errors(&format!(
        r#"{FLOOR_SCAFFOLD}
/** @param non-empty-array<int, int> $qtys */
function floorStock(array $qtys): int
{{
    $max = null;
    foreach ($qtys as $qty) {{
        $max = $qty;
    }}
    return takesInt($max);
}}
"#
    ));
}

/// A shape with a required entry has at least that entry to iterate.
#[test]
fn an_array_shape_with_a_required_entry_drops_the_pre_loop_sentinel() {
    assert_no_type_errors(&format!(
        r#"{FLOOR_SCAFFOLD}
/** @param array{{first: int, second?: int}} $qtys */
function floorStock(array $qtys): int
{{
    $max = null;
    foreach ($qtys as $qty) {{
        $max = $qty;
    }}
    return takesInt($max);
}}
"#
    ));
}

/// Negative control: nothing proves this array has entries, so the loop
/// may not run and the sentinel survives it.
#[test]
fn an_unproven_iterable_keeps_the_pre_loop_sentinel() {
    assert_type_error(&format!(
        r#"{FLOOR_SCAFFOLD}
/** @param array<int, int> $qtys */
function floorStock(array $qtys): int
{{
    $max = null;
    foreach ($qtys as $qty) {{
        $max = $qty;
    }}
    return takesInt($max);
}}
"#
    ));
}

/// Negative control: an all-optional shape can be the empty array, so it
/// proves nothing about whether the body runs.
#[test]
fn an_all_optional_array_shape_keeps_the_pre_loop_sentinel() {
    assert_type_error(&format!(
        r#"{FLOOR_SCAFFOLD}
/** @param array{{first?: int}} $qtys */
function floorStock(array $qtys): int
{{
    $max = null;
    foreach ($qtys as $qty) {{
        $max = $qty;
    }}
    return takesInt($max);
}}
"#
    ));
}

/// Unsetting the entry a `non-empty-array` guarantee relies on empties it
/// out one element at a time, so the loop can no longer be assumed to run
/// and the pre-loop sentinel survives.
#[test]
fn unsetting_an_element_drops_the_non_empty_array_guarantee() {
    assert_type_error(&format!(
        r#"{FLOOR_SCAFFOLD}
/** @param non-empty-array<int, int> $qtys */
function floorStock(array $qtys): int
{{
    unset($qtys[0]);
    $max = null;
    foreach ($qtys as $qty) {{
        $max = $qty;
    }}
    return takesInt($max);
}}
"#
    ));
}

/// Unsetting a shape's only required entry leaves only optional ones, so
/// the shape no longer proves the loop body runs.
#[test]
fn unsetting_a_shapes_required_entry_drops_the_pre_loop_sentinel_proof() {
    assert_type_error(&format!(
        r#"{FLOOR_SCAFFOLD}
/** @param array{{first: int, second?: int}} $qtys */
function floorStock(array $qtys): int
{{
    unset($qtys['first']);
    $max = null;
    foreach ($qtys as $qty) {{
        $max = $qty;
    }}
    return takesInt($max);
}}
"#
    ));
}

/// Negative control: unsetting one required entry off a shape with two
/// leaves the other required entry intact, so the loop still provably runs.
#[test]
fn unsetting_one_of_two_required_shape_entries_keeps_the_other_proof() {
    assert_no_type_errors(&format!(
        r#"{FLOOR_SCAFFOLD}
/** @param array{{first: int, second: int}} $qtys */
function floorStock(array $qtys): int
{{
    unset($qtys['first']);
    $max = null;
    foreach ($qtys as $qty) {{
        $max = $qty;
    }}
    return takesInt($max);
}}
"#
    ));
}

// ─── An assertion tag resolves its type where it was written ───────────────

const VENDOR_ASSERT_SCAFFOLD: &str = r#"<?php
namespace Vendor\Events;

interface Test
{
    /** @phpstan-assert-if-true TestMethod $this */
    public function isTestMethod(): bool;
}

final class TestMethod implements Test
{
    public function isTestMethod(): bool { return true; }

    public function className(): string { return 'x'; }
}
"#;

/// The unqualified `TestMethod` in the tag names
/// `Vendor\Events\TestMethod`, the way PHP resolves every other
/// unqualified name in the file that declares it.  The call site's own
/// namespace has nothing to do with it.
#[test]
fn an_assertion_tag_resolves_its_type_against_the_declaring_namespace() {
    let messages = type_diagnostics(&format!(
        r#"{VENDOR_ASSERT_SCAFFOLD}
namespace App;

use Vendor\Events\Test;

function takesString(string $s): void {{}}

function f(Test $test): void
{{
    if (!$test->isTestMethod()) {{
        return;
    }}
    takesString($test->className());
}}
"#
    ));
    assert!(
        messages.is_empty(),
        "the tag's type resolves in its own namespace, got: {messages:?}"
    );
}

/// `is_a($s, C::class, true)` on a subject that can only hold a string
/// proves a `class-string<C>`.  Adding `C` itself would put an object
/// alternative into a value that is definitely a string.
#[test]
fn is_a_with_allow_string_on_a_string_subject_proves_only_a_class_string() {
    let messages = type_diagnostics(
        r#"<?php
namespace Repro;

class Base {}

/** @param class-string<Base> $cls */
function initialize(string $cls): void {}

function f(string $name): void
{
    if (!is_a($name, Base::class, true)) {
        return;
    }
    initialize($name);
}
"#,
    );
    assert!(
        messages.is_empty(),
        "the subject stays a string, got: {messages:?}"
    );
}

const B228_SCAFFOLD: &str = r#"<?php
namespace Repro;

class ClassReflection { public function isFinal(): bool { return true; } }

interface Invoker { public function invoke(): void; }

interface Scope
{
    /** @phpstan-assert-if-true !null $this->getClassReflection() */
    public function isInClass(): bool;

    public function getClassReflection(): ?ClassReflection;
}

function useClass(ClassReflection $r): void {}
"#;

/// The receiver of the guard can be a property, not just a local: a
/// scope held in a field is guarded exactly the same way.
#[test]
fn an_assert_tag_narrows_through_a_property_receiver() {
    assert_no_type_errors(&format!(
        r#"{B228_SCAFFOLD}
class Holder {{
    private Scope $scope;

    public function __construct(Scope $s) {{ $this->scope = $s; }}

    public function f(): void
    {{
        if (!$this->scope->isInClass()) {{
            return;
        }}
        $reflection = $this->scope->getClassReflection();
        useClass($reflection);
    }}
}}
"#
    ));
}

/// An intersection-typed receiver carries one entry per member, so the
/// member that declares the tag has to be found among them rather than
/// by looking up the joined `A&B` text as a class name.
#[test]
fn an_assert_tag_narrows_through_an_intersection_typed_receiver() {
    assert_no_type_errors(&format!(
        r#"{B228_SCAFFOLD}
function f(Scope&Invoker $scope): void
{{
    if (!$scope->isInClass()) {{
        return;
    }}
    $reflection = $scope->getClassReflection();
    useClass($reflection);
}}
"#
    ));
}

/// The proof holds for every later occurrence of the guarded call, not
/// just the first.  Evaluating the call is what the proof is about, so
/// it must not count as the event that invalidates it.
#[test]
fn a_guarded_call_stays_narrowed_across_repeated_uses() {
    assert_no_type_errors(&format!(
        r#"{B228_SCAFFOLD}
function f(Scope $scope): void
{{
    if (!$scope->isInClass()) {{
        return;
    }}
    useClass($scope->getClassReflection());
    useClass($scope->getClassReflection());
    useClass($scope->getClassReflection());
}}
"#
    ));
}

/// The same across a branch boundary: a use inside a nested `if` and
/// another after it.
#[test]
fn a_guarded_call_survives_a_nested_branch() {
    assert_no_type_errors(&format!(
        r#"{B228_SCAFFOLD}
class Stmt2 {{ public ?string $type = null; }}

function f(Scope $scope, Stmt2 $node): void
{{
    if (!$scope->isInClass()) {{
        throw new \\RuntimeException('x');
    }}
    if ($node->type !== null) {{
        useClass($scope->getClassReflection());
    }}
    useClass($scope->getClassReflection());
}}
"#
    ));
}

/// A `foreach` whose body guards with `continue` keeps the proof for the
/// rest of that iteration, including after an earlier `continue` guard
/// and a statement before the jump.
#[test]
fn a_continue_guard_keeps_the_proof_for_the_rest_of_the_iteration() {
    assert_no_type_errors(&format!(
        r#"{B228_SCAFFOLD}
/** @param list<int> $items */
function f(Scope $scope, array $items): void
{{
    $errors = [];
    foreach ($items as $item) {{
        if ($item === 0) {{
            continue;
        }}
        if (!$scope->isInClass()) {{
            $errors[] = 'x';
            continue;
        }}
        useClass($scope->getClassReflection());
        useClass($scope->getClassReflection());
    }}
    echo count($errors);
}}
"#
    ));
}

/// An impure call on the receiver still drops what it could have
/// changed: the row `$stmt->fetch()` was checked on is not the row a
/// later `$stmt->execute()` leaves behind.
#[test]
fn an_impure_call_on_the_receiver_still_drops_the_other_proofs() {
    let messages = type_diagnostics(
        r#"<?php
namespace Repro;

class Row {}

class Stmt
{
    public function fetch(): ?Row { return null; }

    public function execute(): void {}
}

function useRow(Row $row): void {}

function f(Stmt $stmt): void
{
    if ($stmt->fetch() === null) {
        return;
    }
    $stmt->execute();
    useRow($stmt->fetch());
}
"#,
    );
    assert_eq!(
        messages.len(),
        1,
        "execute() moved the statement past the checked row, got: {messages:?}"
    );
}

/// A predicate that carries the tag can sit anywhere in an `&&` chain:
/// the whole condition holding means every operand did.
#[test]
fn an_assert_tag_in_a_later_and_operand_narrows_the_branch() {
    assert_no_type_errors(&format!(
        r#"{B228_SCAFFOLD}
function f(Scope $scope, string $name): void
{{
    if (
        $name === 'static'
        && $scope->isInClass()
    ) {{
        useClass($scope->getClassReflection());
    }}
}}
"#
    ));
}

/// The same in an `elseif`, which is how the check is usually written
/// once there is more than one class keyword to handle.
#[test]
fn an_assert_tag_in_an_elseif_and_chain_narrows_the_branch() {
    assert_no_type_errors(&format!(
        r#"{B228_SCAFFOLD}
function f(Scope $scope, string $name): void
{{
    if ($name === 'self') {{
        echo 'self';
    }} elseif ($name === 'static' && $scope->isInClass()) {{
        useClass($scope->getClassReflection());
    }}
}}
"#
    ));
}

/// An `||` proves nothing: the branch can be entered without the
/// predicate having held.
#[test]
fn an_assert_tag_in_an_or_chain_proves_nothing() {
    let messages = type_diagnostics(&format!(
        r#"{B228_SCAFFOLD}
function f(Scope $scope, string $name): void
{{
    if ($name === 'static' || $scope->isInClass()) {{
        useClass($scope->getClassReflection());
    }}
}}
"#
    ));
    assert_eq!(
        messages.len(),
        1,
        "an `||` branch can be entered without the predicate, got: {messages:?}"
    );
}

/// A class that implements the predicate's interface without repeating
/// its docblock inherits the contract's tags: `$this->isInClass()` inside
/// the implementation proves what the interface promised.
#[test]
fn an_assert_tag_is_inherited_from_an_implemented_interface() {
    assert_no_type_errors(&format!(
        r#"{B228_SCAFFOLD}
class MutatingScope implements Scope
{{
    private ?ClassReflection $reflection = null;

    public function isInClass(): bool
    {{
        return $this->reflection !== null;
    }}

    public function getClassReflection(): ?ClassReflection
    {{
        return $this->reflection;
    }}

    public function resolve(): void
    {{
        if (!$this->isInClass()) {{
            return;
        }}
        useClass($this->getClassReflection());
    }}
}}
"#
    ));
}

/// A sibling branch that tests the same predicate must not corrupt the
/// fact for the branches after it: the inverse of `A && guard()` is an
/// alternative, and the alternative where `A` failed says nothing about
/// what `guard()` proves.
#[test]
fn a_sibling_branch_testing_the_same_predicate_does_not_corrupt_the_next() {
    assert_no_type_errors(&format!(
        r#"{B228_SCAFFOLD}
function f(Scope $scope, string $name): void
{{
    if ($name === 'self' && $scope->isInClass()) {{
        useClass($scope->getClassReflection());
    }} elseif ($name === 'static' && $scope->isInClass()) {{
        useClass($scope->getClassReflection());
    }} elseif ($name === 'parent' && $scope->isInClass()) {{
        useClass($scope->getClassReflection());
    }}
}}
"#
    ));
}

// ─── A loop that bails on a bad entry proves the whole collection ───────────

const PRE_VALIDATION_SCAFFOLD: &str = r#"<?php
namespace Repro;

class Name {}
class Expr
{
    public function name(): ?Name { return null; }
}
class ClassConstFetch extends Expr
{
    public function name(): Name { return new Name(); }
}
class Arm
{
    /** @var Expr[] */
    public array $conds = [];
}
function useName(Name $name): void {}
"#;

/// The pre-validation idiom: one loop rejects the whole collection the
/// moment an entry fails the check, so reaching the code after it means
/// every entry passed.  A second loop over the same expression is what the
/// idiom exists for, and it may read the proven member without checking
/// again.
#[test]
fn a_loop_that_bails_out_past_itself_narrows_the_collection_it_iterated() {
    assert_no_type_errors(&format!(
        r#"{PRE_VALIDATION_SCAFFOLD}
/** @param Arm[] $arms */
function f(array $arms): void
{{
    foreach ($arms as $arm) {{
        foreach ($arm->conds as $cond) {{
            if (!$cond instanceof ClassConstFetch) {{
                break 2;
            }}
        }}
        foreach ($arm->conds as $other) {{
            useName($other->name());
        }}
    }}
}}
"#
    ));
}

/// A `return` guard proves it for the rest of the function the same way.
#[test]
fn a_loop_that_returns_on_a_bad_entry_narrows_the_collection_it_iterated() {
    assert_no_type_errors(&format!(
        r#"{PRE_VALIDATION_SCAFFOLD}
function f(Arm $arm): void
{{
    foreach ($arm->conds as $cond) {{
        if (!$cond instanceof ClassConstFetch) {{
            return;
        }}
    }}
    foreach ($arm->conds as $other) {{
        useName($other->name());
    }}
}}
"#
    ));
}

/// Negative control: a plain `break` jumps to exactly the code the claim
/// would be made about, so the entries after it were never checked.
#[test]
fn a_loop_that_only_breaks_itself_proves_nothing_about_the_collection() {
    assert_type_error(&format!(
        r#"{PRE_VALIDATION_SCAFFOLD}
function f(Arm $arm): void
{{
    foreach ($arm->conds as $cond) {{
        if (!$cond instanceof ClassConstFetch) {{
            break;
        }}
    }}
    foreach ($arm->conds as $other) {{
        useName($other->name());
    }}
}}
"#
    ));
}

/// Negative control: `continue` skips the entry, not the rest of the
/// program, so the collection still holds the entries it skipped.
#[test]
fn a_loop_that_continues_past_a_bad_entry_proves_nothing_about_the_collection() {
    assert_type_error(&format!(
        r#"{PRE_VALIDATION_SCAFFOLD}
function f(Arm $arm): void
{{
    foreach ($arm->conds as $cond) {{
        if (!$cond instanceof ClassConstFetch) {{
            continue;
        }}
    }}
    foreach ($arm->conds as $other) {{
        useName($other->name());
    }}
}}
"#
    ));
}

/// Negative control: an `else` makes the `if` a branch rather than a
/// guard, so falling out of its bottom says nothing about the condition.
#[test]
fn a_loop_whose_check_has_an_else_branch_proves_nothing_about_the_collection() {
    assert_type_error(&format!(
        r#"{PRE_VALIDATION_SCAFFOLD}
function f(Arm $arm): void
{{
    foreach ($arm->conds as $cond) {{
        if (!$cond instanceof ClassConstFetch) {{
            return;
        }} else {{
            echo 'ok';
        }}
    }}
    foreach ($arm->conds as $other) {{
        useName($other->name());
    }}
}}
"#
    ));
}

/// The proof is about the collection, so writing to it afterwards drops it.
#[test]
fn writing_to_the_collection_drops_what_the_loop_proved_about_it() {
    assert_type_error(&format!(
        r#"{PRE_VALIDATION_SCAFFOLD}
/** @param Expr[] $fresh */
function f(Arm $arm, array $fresh): void
{{
    foreach ($arm->conds as $cond) {{
        if (!$cond instanceof ClassConstFetch) {{
            return;
        }}
    }}
    $arm->conds = $fresh;
    foreach ($arm->conds as $other) {{
        useName($other->name());
    }}
}}
"#
    ));
}

// ─── Falling out of the bottom of an if/elseif chain ────────────────────────

/// Every condition in the chain was false on the fall-through path, so
/// each one's inverse holds there. An `elseif` used to make the leading
/// condition's inverse go missing: the guard-clause pass declines to run
/// once there is an `elseif`, and the implicit-else path never learned it.
#[test]
fn falling_past_an_elseif_chain_inverts_every_condition_in_it() {
    assert_no_type_errors(
        r#"<?php
namespace Repro;

class Tip
{
    public function text(): string { return ''; }
}

function useTip(Tip $tip): string { return $tip->text(); }

function f(?Tip $a, ?Tip $b): int
{
    if ($a === null) {
        return 1;
    } elseif ($b === null) {
        return -1;
    }

    return strcmp(useTip($a), useTip($b));
}
"#,
    );
}

/// The same shape with the guard written on an array offset, which is how
/// a comparison callback tests an optional tuple member.
#[test]
fn falling_past_an_elseif_chain_keeps_an_isset_proof_on_an_offset() {
    assert_no_type_errors(
        r#"<?php
namespace Repro;

class Tip
{
    public function text(): string { return ''; }
}

function useTip(Tip $tip): string { return $tip->text(); }

/**
 * @param array{0: string, 2?: Tip|null} $a
 * @param array{0: string, 2?: Tip|null} $b
 */
function f(array $a, array $b): int
{
    if (!isset($a[2])) {
        if (!isset($b[2])) {
            return 0;
        }

        return 1;
    } elseif (!isset($b[2])) {
        return -1;
    }

    return strcmp(useTip($a[2]), useTip($b[2]));
}
"#,
    );
}

/// Negative control: an `elseif` body that falls through instead of
/// exiting reaches the bottom with its own condition *true*, so nothing
/// downstream may assume the inverse.
#[test]
fn an_elseif_that_falls_through_does_not_prove_its_condition_false() {
    assert_type_error(
        r#"<?php
namespace Repro;

class Tip
{
    public function text(): string { return ''; }
}

function useTip(Tip $tip): string { return $tip->text(); }

function f(?Tip $a, ?Tip $b): string
{
    if ($a === null) {
        return 'first';
    } elseif ($b === null) {
        echo 'noted';
    }

    return useTip($b);
}
"#,
    );
}

// ─── Reading an offset of a union of array shapes ───────────────────────────

/// Every alternative contributes its own entry: offset 1 of
/// `array{string, null}|array{string, Err}` is `null|Err`, so the guard
/// that rules out `null` leaves `Err`. Taking only the first alternative's
/// entry left the guard nothing to remove and the read reported `null`.
#[test]
fn an_offset_of_a_union_of_shapes_unions_every_alternatives_entry() {
    assert_no_type_errors(
        r#"<?php
namespace Repro;

class Err
{
    public function message(): string { return ''; }
}

function useMessage(string $m): void {}

/** @param list<array{string, null}|array{string, Err}> $rows */
function f(array $rows): void
{
    foreach ($rows as $row) {
        if ($row[1] === null) {
            continue;
        }
        useMessage($row[1]->message());
    }
}
"#,
    );
}

// ─── Guarding a call and reading it again ───────────────────────────────────

/// A guard on a call's result describes every later occurrence of that
/// call, and a parameterised array return type is no exception. Seeding the
/// call's key was skipped for any type that resolved to no class, which
/// took `list<int>|null` with it and left the guard nothing to narrow.
#[test]
fn a_guard_on_a_call_returning_a_parameterised_array_narrows_the_next_one() {
    assert_no_type_errors(
        r#"<?php
namespace Repro;

class Result
{
    /** @return array<string, array<string>>|null */
    public function getDependencies(): ?array { return null; }
}

/** @param array<string, array<string>> $dependencies */
function useDependencies(array $dependencies): void {}

function f(Result $result): void
{
    if ($result->getDependencies() !== null) {
        useDependencies($result->getDependencies());
    }
}
"#,
    );
}

/// An override that restates no return type says what its ancestor said,
/// which is the whole point of the `{@inheritDoc}` it carries. Reading the
/// override's silence as "no type known" left the guard on the call with
/// nothing to narrow.
#[test]
fn a_guard_narrows_a_call_whose_return_type_is_only_declared_upstream() {
    assert_no_type_errors(
        r#"<?php
namespace Repro;

abstract class BaseReflection
{
    /** @return string|false */
    public function getFileName() { return false; }
}

class ReflectionAdapter extends BaseReflection
{
    /**
     * {@inheritDoc}
     */
    public function getFileName() { return false; }
}

function useFileName(?string $file): void {}

function f(ReflectionAdapter $reflection): void
{
    if ($reflection->getFileName() !== false) {
        useFileName($reflection->getFileName());
    }
}
"#,
    );
}

/// Reading a second accessor on the same object is not an event that
/// unproves a guard on the first. Only a call that changes state behind the
/// receiver is, which is what its return type (or an `@impure` tag) says.
#[test]
fn a_second_getter_on_the_receiver_leaves_the_first_ones_guard_standing() {
    assert_no_type_errors(
        r#"<?php
namespace Repro;

class Reflection
{
    /** @return string|false */
    public function getFileName() { return false; }

    /** @return string|false */
    public function getDocComment() { return false; }
}

function useFileName(?string $file): void {}

function f(Reflection $reflection): void
{
    if ($reflection->getFileName() !== false && $reflection->getDocComment() !== false) {
        $doc = $reflection->getDocComment();
        useFileName($reflection->getFileName());
        useFileName($doc);
    }
}
"#,
    );
}

/// The same receiver, but the intervening call hands nothing back: it was
/// made for its effect, so the guard on the earlier accessor no longer
/// describes what the object holds.
#[test]
fn a_call_returning_nothing_does_drop_the_receivers_guard() {
    assert_type_error(
        r#"<?php
namespace Repro;

class Reflection
{
    /** @return string|false */
    public function getFileName() { return false; }

    public function reload(): void {}
}

function useFileName(?string $file): void {}

function f(Reflection $reflection): void
{
    if ($reflection->getFileName() !== false) {
        $reflection->reload();
        useFileName($reflection->getFileName());
    }
}
"#,
    );
}

// ─── Seed-if-absent array writes ────────────────────────────────────────────

const SEED_IF_ABSENT_SCAFFOLD: &str = r#"<?php
namespace Repro;

class Total {
    public function add(int|self $v): self { return $this; }
}

function makeTotal(mixed $v): Total { return new Total(); }
"#;

/// `if (!isset($tmp[$key])) { $tmp[$key] = 0; }` proves the element
/// present on both paths out of the guard: the then-branch just wrote it,
/// and the fall-through only runs when `isset` already said it was there.
/// The write goes through a variable key, so it must overwrite the
/// synthetic scope entry the guard narrowed to null, or that stale null
/// resurfaces once the branches rejoin.
#[test]
fn a_variable_keyed_write_after_a_true_isset_guard_leaves_no_null_behind() {
    assert_no_type_errors(&format!(
        r#"{SEED_IF_ABSENT_SCAFFOLD}
/** @param array<string, mixed> $row */
function totals(array $row): void
{{
    $tmp = [];
    foreach ($row as $key => $value) {{
        if (!isset($tmp[$key])) {{
            $tmp[$key] = 0;
        }}
        $tmp[$key] = makeTotal($value)->add($tmp[$key]);
    }}
}}
"#
    ));
}

// ─── A closure captures the paths read through what it captures ─────────────

const CAPTURE_SCAFFOLD: &str = r#"<?php
namespace Repro;

class Node {}

class Param {
    public ?Node $type = null;
}

function acceptNode(Node $n): void {}
"#;

/// The guard above the closure recorded its proof under `$param->type`,
/// and `use ($param)` captures the value that path is read through — so
/// the body sees the narrowed path, not the declaration.
#[test]
fn a_closure_keeps_the_narrowing_of_a_path_it_captures() {
    assert_no_type_errors(&format!(
        r#"{CAPTURE_SCAFFOLD}
/** @param Param[] $params */
function check(array $params): void
{{
    foreach ($params as $param) {{
        if ($param->type === null) {{
            continue;
        }}
        $run = static function () use ($param): void {{
            acceptNode($param->type);
        }};
        $run();
    }}
}}
"#
    ));
}

/// The same for `$this`, which a closure captures without naming.
#[test]
fn a_closure_keeps_the_narrowing_of_a_path_read_through_this() {
    assert_no_type_errors(&format!(
        r#"{CAPTURE_SCAFFOLD}
class Holder {{
    public ?Node $node = null;

    public function check(): void
    {{
        if ($this->node === null) {{
            return;
        }}
        $run = function (): void {{
            acceptNode($this->node);
        }};
        $run();
    }}
}}
"#
    ));
}

/// Nothing above the closure proved anything, so the captured path keeps
/// the `null` its declaration allows.
#[test]
fn an_unguarded_capture_keeps_the_declared_null() {
    assert_type_error(&format!(
        r#"{CAPTURE_SCAFFOLD}
function check(Param $param): void
{{
    $run = static function () use ($param): void {{
        acceptNode($param->type);
    }};
    $run();
}}
"#
    ));
}

// ─── A negated check on a path the same chain narrows the receiver of ───────

/// `$expr->name` can only be looked up once `$expr instanceof FuncCall`
/// has been applied, so the negated `instanceof` on it has to be read
/// after the receiver's own narrowing rather than before it.
#[test]
fn a_negated_check_on_a_path_narrowed_by_an_earlier_conjunct_applies() {
    assert_no_type_errors(
        r#"<?php
namespace Repro;

class Name {}
class Expr {}
class FuncCall extends Expr {
    /** @var Name|Expr */
    public $name;
}

function acceptExpr(Expr $e): void {}

function process(Expr $expr): void
{
    if ($expr instanceof FuncCall && !$expr->name instanceof Name) {
        acceptExpr($expr->name);
    }
}
"#,
    );
}
