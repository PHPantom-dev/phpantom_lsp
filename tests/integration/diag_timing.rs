//! Diagnostic timing tests with self-contained fixtures.
//!
//! The two phpstan-fixture benchmarks are `#[ignore]`d by default because
//! they take 10-20 s each.  Run them explicitly with:
//!
//!   cargo nextest run -E 'test(diag_timing::time_diagnostics)' --run-ignored all
//!
//! or with the built-in runner:
//!
//!   cargo test --release -p phpantom_lsp --test integration diag_timing -- --ignored --nocapture
//!
//! The `warm_cache` tests simulate the real editing scenario: diagnostics
//! run once (cold, populates the resolved-class cache), then the user
//! edits a single file and diagnostics run again.  With targeted cache
//! invalidation, classes from unedited files stay cached and the second
//! pass is significantly faster.

use crate::common::{
    create_psr4_workspace, create_test_backend, create_test_backend_with_full_stubs,
};
use std::time::Instant;
use tower_lsp::lsp_types::NumberOrString;

/// Regression test for variable-type-caching in deprecated diagnostics.
///
/// Without caching, every `$var->method()` call triggers a separate
/// variable-type resolution pass.  With N accesses on the same variable
/// this becomes O(N * parse), which blows up quickly.  The fix (a
/// per-variable cache keyed by `(var_name, enclosing_class)`) collapses
/// this to O(k * parse) where k is the number of distinct variables.
///
/// This test creates a class with many deprecated methods and a consumer
/// that calls them repeatedly on the same variable.  If the cache regresses,
/// the test will exceed its time budget.
#[tokio::test]
async fn deprecated_diagnostics_variable_cache_regression() {
    // Build a class with 30 deprecated methods and a consumer that calls
    // each one twice on the same $svc variable = 60 member accesses that
    // all resolve to the same variable type.
    let mut php = String::from("<?php\nclass Service {\n    public function ok(): void {}\n");
    for i in 0..30 {
        php.push_str(&format!(
            "    /** @deprecated Use ok() instead */\n    public function old{}(): void {{}}\n",
            i
        ));
    }
    php.push_str(
        "}\n\nclass Consumer {\n    public function run(): void {\n        $svc = new Service();\n",
    );
    for i in 0..30 {
        php.push_str(&format!("        $svc->old{}();\n", i));
        php.push_str(&format!("        $svc->old{}();\n", i));
    }
    php.push_str("    }\n}\n");

    let uri = "file:///test/service.php";
    let backend = create_test_backend();
    backend.update_ast(uri, &php);

    let start = Instant::now();
    let mut out = Vec::new();
    backend.collect_deprecated_diagnostics(uri, &php, &mut out);
    let elapsed = start.elapsed();

    eprintln!();
    eprintln!("=== Deprecated diagnostics variable-cache regression ===");
    eprintln!(
        "  60 member accesses on same $svc: {:>10.3?}  ({} diagnostics)",
        elapsed,
        out.len()
    );
    eprintln!();

    // Each of the 30 deprecated methods is called twice = 60 diagnostics.
    assert_eq!(out.len(), 60, "expected 60 deprecated diagnostics");

    // Budget: 5 s in debug, 1 s in release.  Without the cache this
    // takes 20+ s on a ~60-access file; with caching it's < 1 s.
    let budget_secs = if cfg!(debug_assertions) { 5.0 } else { 1.0 };
    assert!(
        elapsed.as_secs_f64() < budget_secs,
        "Deprecated diagnostics took {:.3?} which exceeds the {:.0} s budget. \
         The per-variable type cache may have regressed.",
        elapsed,
        budget_secs,
    );
}

/// Verify that a variable chain within the backward scanner's depth limit
/// resolves correctly for diagnostics.
///
/// A 5-deep variable chain (`$a = new Foo; $b = $a->next(); ... $e`)
/// where each `->next()` returns the same class.  All member accesses
/// should resolve without false-positive diagnostics.  The unknown member
/// `$e->nonexistent` should be flagged.
#[test]
fn variable_chain_no_false_positive() {
    let php = r#"<?php
class Chain {
    public string $name = 'ok';
    public function next(): Chain { return new Chain(); }
}

class Consumer {
    public function run(): void {
        $a = new Chain();
        $b = $a->next();
        $c = $b->next();
        $d = $c->next();
        $e = $d->next();
        // All of these should resolve — no unknown_member diagnostics.
        $e->name;
        $d->name;
        $a->name;
        $a->next();
        $e->next();
        // This one IS unknown — should produce a diagnostic.
        $e->nonexistent;
    }
}
"#;

    let uri = "file:///test/chain.php";
    let backend = create_test_backend_with_full_stubs();
    backend.update_ast(uri, php);

    let mut out = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, php, &mut out);

    // Filter to unknown_member diagnostics only (exclude scalar_member_access etc.).
    let unknown_member: Vec<_> = out
        .iter()
        .filter(|d| {
            d.code
                .as_ref()
                .is_some_and(|c| matches!(c, NumberOrString::String(s) if s == "unknown_member"))
        })
        .collect();

    // Only `$e->nonexistent` should be flagged.
    assert_eq!(
        unknown_member.len(),
        1,
        "Expected exactly 1 unknown_member diagnostic for $e->nonexistent, got {}: {:?}",
        unknown_member.len(),
        unknown_member
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    assert!(
        unknown_member[0].message.contains("nonexistent"),
        "Diagnostic should be about 'nonexistent', got: {}",
        unknown_member[0].message
    );
}

/// Verify that foreach bindings resolve correctly for diagnostics.
///
/// The foreach value variable should resolve to the iterable's element
/// type, and member accesses on it should not produce false positives.
#[test]
fn foreach_binding_no_false_positive() {
    let php = r#"<?php
class Item {
    public string $label = '';
    public function process(): void {}
}

class Container {
    /** @return Item[] */
    public function items(): array { return []; }
}

class Worker {
    public function run(Container $c): void {
        foreach ($c->items() as $item) {
            $item->label;
            $item->process();
            $item->doesNotExist;
        }
    }
}
"#;

    let uri = "file:///test/foreach.php";
    let backend = create_test_backend_with_full_stubs();
    backend.update_ast(uri, php);

    let mut out = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, php, &mut out);

    let unknown_member: Vec<_> = out
        .iter()
        .filter(|d| {
            d.code
                .as_ref()
                .is_some_and(|c| matches!(c, NumberOrString::String(s) if s == "unknown_member"))
        })
        .collect();

    assert_eq!(
        unknown_member.len(),
        1,
        "Expected 1 diagnostic for $item->doesNotExist, got {}: {:?}",
        unknown_member.len(),
        unknown_member
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    assert!(
        unknown_member[0].message.contains("doesNotExist"),
        "Diagnostic should be about 'doesNotExist', got: {}",
        unknown_member[0].message
    );
}

/// Verify that a nullable `self` parameter resolves correctly after a
/// null guard clause (`if ($x === null) { return; }`).
///
/// Regression test: the forward walker must strip null from the
/// parameter's type after the guard clause so that member accesses on
/// the non-null path resolve against the class.
#[test]
fn nullable_self_param_guard_clause_no_false_positive() {
    let php = r#"<?php
class TimePeriod {
    public function __construct(
        private int $length,
        private string $unit,
    ) {}

    public function isEqualTo(?self $other): bool {
        if ($other === null) {
            return false;
        }

        return $this->length === $other->getLength() && $this->unit === $other->getUnit();
    }

    public function getLength(): int { return $this->length; }
    public function getUnit(): string { return $this->unit; }
}
"#;

    let uri = "file:///test/nullable_self.php";
    let backend = create_test_backend_with_full_stubs();
    backend.update_ast(uri, php);

    let mut out = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, php, &mut out);

    let unknown: Vec<_> = out
        .iter()
        .filter(|d| {
            d.code
                .as_ref()
                .is_some_and(|c| matches!(c, NumberOrString::String(s) if s == "unknown_member"))
        })
        .collect();

    assert!(
        unknown.is_empty(),
        "Expected no unknown_member for $other->getLength()/getUnit(), got {}: {:?}",
        unknown.len(),
        unknown.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Verify that multiple variables in the same method body all resolve
/// independently when each has a different type.
#[test]
fn multiple_variables_different_types_no_false_positive() {
    let php = r#"<?php
class Dog {
    public string $breed = '';
    public function bark(): void {}
}

class Cat {
    public string $color = '';
    public function meow(): void {}
}

class Vet {
    public function examine(Dog $dog, Cat $cat): void {
        $d = $dog;
        $c = $cat;
        $d->breed;
        $d->bark();
        $c->color;
        $c->meow();
        // Cross-type accesses should be flagged.
        $d->meow;
        $c->bark;
    }
}
"#;

    let uri = "file:///test/multi.php";
    let backend = create_test_backend_with_full_stubs();
    backend.update_ast(uri, php);

    let mut out = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, php, &mut out);

    let unknown_member: Vec<_> = out
        .iter()
        .filter(|d| {
            d.code
                .as_ref()
                .is_some_and(|c| matches!(c, NumberOrString::String(s) if s == "unknown_member"))
        })
        .collect();

    // $d->meow and $c->bark should be flagged.
    assert_eq!(
        unknown_member.len(),
        2,
        "Expected 2 unknown_member diagnostics, got {}: {:?}",
        unknown_member.len(),
        unknown_member
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

/// Verify that parameter types from a generic interface (`@implements`)
/// resolve correctly through the merged class in the forward walker.
///
/// When a class declares `@implements CastsAttributes<Decimal, Decimal>`
/// and the interface method `set()` has `mixed $value`, the merged class
/// should substitute the generic parameter so `$value` resolves to
/// `Decimal`.  Without this, the forward walker seeds `$value` as `mixed`
/// and member accesses on it produce false positives.
#[test]
fn generic_interface_param_resolution_no_false_positive() {
    let php = r#"<?php
/**
 * @template TGet
 * @template TSet
 */
interface CastsAttributes {
    /** @param TSet $value */
    public function set(mixed $value): ?string;
}

class Decimal {
    public function toFixed(int $scale): string { return '0'; }
    public function floor(): self { return new self(); }
}

/**
 * @implements CastsAttributes<Decimal, Decimal>
 */
final class DecimalCast implements CastsAttributes {
    public function set(mixed $value): ?string {
        $floor = $value->floor();
        return $value->toFixed(2);
    }
}
"#;

    let uri = "file:///test/casts.php";
    let backend = create_test_backend_with_full_stubs();
    backend.update_ast(uri, php);

    let mut out = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, php, &mut out);

    // Filter to unknown_member and unresolved_member_access.
    let relevant: Vec<_> = out
        .iter()
        .filter(|d| {
            d.code.as_ref().is_some_and(|c| {
                matches!(c, NumberOrString::String(s) if s == "unknown_member" || s == "unresolved_member_access")
            })
        })
        .collect();

    assert!(
        relevant.is_empty(),
        "Expected no diagnostics for generic interface param resolution, got {}: {:?}",
        relevant.len(),
        relevant.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Verify that `is_object()` type-guard narrowing works in the diagnostic
/// scope cache (forward walker).
///
/// When `$data` starts as `mixed` and the condition is `is_object($data) &&
/// property_exists($data, 'error_link')`, the forward walker should narrow
/// `$data` to `object` both within the `&&` chain and inside the `if` body.
#[test]
fn is_object_type_guard_no_false_positive() {
    let php = r#"<?php
function test(mixed $data): ?string {
    if (is_object($data) && property_exists($data, 'error_link') && is_string($data->error_link)) {
        return stripslashes($data->error_link);
    }

    return null;
}
"#;

    let uri = "file:///test/guard.php";
    let backend = create_test_backend_with_full_stubs();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    backend.update_ast(uri, php);

    let mut out = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, php, &mut out);

    assert!(
        out.is_empty(),
        "Expected no diagnostics after is_object() guard in && chain, got {}: {:?}",
        out.len(),
        out.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Verify that `is_object()` type-guard narrowing works with a negated
/// early return (guard clause pattern) in the forward walker.
#[test]
fn is_object_guard_clause_no_false_positive() {
    let php = r#"<?php
function test(mixed $data): void {
    if (!is_object($data)) {
        return;
    }
    echo $data->some_property;
    $data->doStuff();
}
"#;

    let uri = "file:///test/guard_clause.php";
    let backend = create_test_backend_with_full_stubs();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    backend.update_ast(uri, php);

    let mut out = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, php, &mut out);

    assert!(
        out.is_empty(),
        "Expected no diagnostics after negated is_object() early return, got {}: {:?}",
        out.len(),
        out.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Verify that compound OR instanceof narrowing works in the forward walker.
///
/// When the condition is `$x instanceof A || $x instanceof B`, the
/// then-body should see `$x` narrowed to `A|B` so that members
/// defined on both classes are found.
#[test]
fn compound_or_instanceof_narrowing_no_false_positive() {
    let php = r#"<?php
class Animal {
    public function speak(): string { return ''; }
}
class Dog extends Animal {
    public function fetch(): string { return 'stick'; }
}
class Cat extends Animal {
    public function purr(): string { return 'purr'; }
}

function test(Animal $pet): string {
    if ($pet instanceof Dog || $pet instanceof Cat) {
        return $pet->speak();
    }
    return '';
}
"#;

    let uri = "file:///test/compound_or.php";
    let backend = create_test_backend_with_full_stubs();
    backend.update_ast(uri, php);

    let mut out = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, php, &mut out);

    let relevant: Vec<_> = out
        .iter()
        .filter(|d| {
            d.code.as_ref().is_some_and(|c| {
                matches!(c, NumberOrString::String(s) if s == "unknown_member" || s == "unresolved_member_access")
            })
        })
        .collect();

    assert!(
        relevant.is_empty(),
        "Expected no diagnostics for compound OR instanceof, got {}: {:?}",
        relevant.len(),
        relevant.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Verify that compound OR guard clause narrowing works.
///
/// When the condition is `!$x instanceof A || <other>` and the body
/// exits, after the if `$x` should be narrowed to `A`.
#[test]
fn compound_or_guard_clause_narrows_instanceof() {
    let php = r#"<?php
interface UserContract {
    public function getId(): int;
}
class User implements UserContract {
    public int $department_id;
    public function getId(): int { return 1; }
}

function test(UserContract $user, ?int $dept_id): int {
    if (! $user instanceof User || $dept_id === null) {
        return 0;
    }
    return $user->department_id;
}
"#;

    let uri = "file:///test/compound_or_guard.php";
    let backend = create_test_backend_with_full_stubs();
    backend.update_ast(uri, php);

    let mut out = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, php, &mut out);

    let relevant: Vec<_> = out
        .iter()
        .filter(|d| {
            d.code.as_ref().is_some_and(|c| {
                matches!(c, NumberOrString::String(s) if s == "unknown_member" || s == "unresolved_member_access")
            })
        })
        .collect();

    assert!(
        relevant.is_empty(),
        "Expected no diagnostics for OR guard clause narrowing, got {}: {:?}",
        relevant.len(),
        relevant.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Verify that `match(true)` arm conditions narrow variables in the
/// arm body via the diagnostic scope cache.
///
/// `match(true) { $x instanceof Foo => $x->method(), ... }` should
/// see `$x` narrowed to `Foo` inside the arm expression.
#[test]
fn match_true_instanceof_narrowing_no_false_positive() {
    let php = r#"<?php
class Dog {
    public function bark(): string { return 'woof'; }
}
class Cat {
    public function purr(): string { return 'purr'; }
}

function describe(Dog|Cat $pet): string {
    return match (true) {
        $pet instanceof Dog => $pet->bark(),
        $pet instanceof Cat => $pet->purr(),
    };
}
"#;

    let uri = "file:///test/match_true.php";
    let backend = create_test_backend_with_full_stubs();
    backend.update_ast(uri, php);

    let mut out = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, php, &mut out);

    let relevant: Vec<_> = out
        .iter()
        .filter(|d| {
            d.code.as_ref().is_some_and(|c| {
                matches!(c, NumberOrString::String(s) if s == "unknown_member" || s == "unresolved_member_access")
            })
        })
        .collect();

    assert!(
        relevant.is_empty(),
        "Expected no diagnostics for match(true) instanceof, got {}: {:?}",
        relevant.len(),
        relevant.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Verify that ternary instanceof narrowing works in the diagnostic
/// scope cache.
///
/// `$x instanceof Foo ? $x->fooMethod() : $x->baseMethod()` should
/// see `$x` narrowed to `Foo` in the then-branch.
#[test]
fn ternary_instanceof_narrowing_no_false_positive() {
    let php = r#"<?php
class Base {
    public function name(): string { return ''; }
}
class Special extends Base {
    public function extra(): string { return 'extra'; }
}

function describe(Base $item): string {
    return $item instanceof Special ? $item->extra() : $item->name();
}
"#;

    let uri = "file:///test/ternary_instanceof.php";
    let backend = create_test_backend_with_full_stubs();
    backend.update_ast(uri, php);

    let mut out = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, php, &mut out);

    let relevant: Vec<_> = out
        .iter()
        .filter(|d| {
            d.code.as_ref().is_some_and(|c| {
                matches!(c, NumberOrString::String(s) if s == "unknown_member" || s == "unresolved_member_access")
            })
        })
        .collect();

    assert!(
        relevant.is_empty(),
        "Expected no diagnostics for ternary instanceof narrowing, got {}: {:?}",
        relevant.len(),
        relevant.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Verify that `assert($data instanceof stdClass)` narrowing survives
/// through multiple intervening `if` blocks with early returns.
///
/// Regression test for Luxplus Shared `RefundCallback`: after the assert
/// on line 33, several `if (!$refund)` / `if (!$payment) { return; }`
/// blocks follow.  The forward walker must preserve the `stdClass`
/// narrowing across all of them so that `$data->status` on a later line
/// does not produce an `unresolved_member_access` diagnostic.
#[test]
fn assert_instanceof_survives_intervening_if_blocks() {
    let php = r#"<?php
use stdClass;
use Vendor\Convert;
use Vendor\RefundService;
use Vendor\PaymentService;
use Vendor\PaymentGateway;
use Vendor\MobilepayRefundCallbackException;

class RefundCallback {
    protected string $data;

    public function handle(): void {
        $data = json_decode($this->data, false, 512, JSON_THROW_ON_ERROR);
        assert($data instanceof stdClass);

        $paymentId = Convert::toString($data->payment_id);
        $amount = Convert::toDecimal($data->amount);
        $gatewayRefundId = Convert::toString($data->refund_id);
        $statusText = Convert::toString($data->status_text ?? '');
        $externalId = Convert::toString($data->external_id ?? null);

        $refund = null;
        $refundService = app()->make(RefundService::class);
        if ($externalId) {
            $refund = $refundService->getRefundById(Convert::toInt($externalId));
        }

        if (!$refund) {
            $payment = app()->make(PaymentService::class)->getPaymentByGatewayAndGatewayPaymentId(PaymentGateway::MOBILEPAY, $paymentId);
            if (!$payment) {
                return;
            }
            $refund = $refundService->getOrCreateMissing($payment, $gatewayRefundId, $amount);
        }

        if (!is_string($data->status)) {
            throw new MobilepayRefundCallbackException('Unknown status: ' . gettype($data->status));
        }

        switch ($data->status) {
            case 'Issued':
                $refundService->setCompleted($refund);
                break;
            case 'Declined':
                $refundService->setFailed($refund, $statusText);
                break;
            default:
                throw new MobilepayRefundCallbackException('Unknown status: ' . $data->status);
        }
    }
}
"#;

    let uri = "file:///test/refund.php";
    let backend = create_test_backend_with_full_stubs();
    backend.update_ast(uri, php);

    let mut out = Vec::new();
    backend.collect_slow_diagnostics(uri, php, &mut out);

    let relevant: Vec<_> = out
        .iter()
        .filter(|d| {
            d.code.as_ref().is_some_and(
                |c| matches!(c, NumberOrString::String(s) if s == "unresolved_member_access"),
            )
        })
        .collect();

    assert!(
        relevant.is_empty(),
        "Expected no unresolved_member_access after assert instanceof with intervening ifs, got {}: {:?}",
        relevant.len(),
        relevant.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Verify that `match(true)` with comma-separated conditions (multiple
/// instanceof checks on the same arm) narrow correctly.
///
/// `match(true) { $x instanceof A, $x instanceof B => $x->shared(), ... }`
/// The comma-separated conditions are an OR — any of them can match.
#[test]
fn match_true_comma_conditions_no_false_positive() {
    let php = r#"<?php
class Subscription {
    public function customer(): ?Customer { return null; }
}
class Payment {
    public function order(): ?Order { return null; }
}
class Customer {
    public string $country = '';
}
class Order {
    public string $country = '';
}

/** @param Subscription|Payment $model */
function getCountry(mixed $model): ?string {
    return match (true) {
        $model instanceof Subscription => $model->customer()?->country,
        $model instanceof Payment => $model->order()?->country,
        default => null,
    };
}
"#;

    let uri = "file:///test/match_comma.php";
    let backend = create_test_backend_with_full_stubs();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    backend.update_ast(uri, php);

    let mut out = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, php, &mut out);

    assert!(
        out.is_empty(),
        "Expected no diagnostics for match(true) comma conditions, got {}: {:?}",
        out.len(),
        out.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Verify that closures with explicitly typed parameters resolve
/// correctly during the diagnostic pass (forward walker walks the
/// closure body and seeds parameter types).
#[test]
fn closure_typed_params_no_false_positive() {
    let php = r#"<?php
class Item {
    public string $name = '';
    public int $price = 0;
}

class Processor {
    /** @param list<Item> $items */
    public function run(array $items): void {
        usort($items, function (Item $a, Item $b): int {
            return $a->price - $b->price;
        });

        array_walk($items, function (Item $item): void {
            echo $item->name;
        });
    }
}
"#;

    let uri = "file:///test/closure_typed.php";
    let backend = create_test_backend_with_full_stubs();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    backend.update_ast(uri, php);

    let mut out = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, php, &mut out);

    assert!(
        out.is_empty(),
        "Expected no diagnostics for closure with typed params, got {}: {:?}",
        out.len(),
        out.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Verify that arrow functions with typed parameters resolve correctly
/// during the diagnostic pass.
#[test]
fn arrow_function_typed_params_no_false_positive() {
    let php = r#"<?php
class Product {
    public string $sku = '';
    public int $stock = 0;
}

class Warehouse {
    /** @param list<Product> $products */
    public function findInStock(array $products): array {
        return array_filter($products, fn(Product $p) => $p->stock > 0);
    }

    /** @param list<Product> $products */
    public function skus(array $products): array {
        return array_map(fn(Product $p) => $p->sku, $products);
    }
}
"#;

    let uri = "file:///test/arrow_typed.php";
    let backend = create_test_backend_with_full_stubs();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    backend.update_ast(uri, php);

    let mut out = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, php, &mut out);

    assert!(
        out.is_empty(),
        "Expected no diagnostics for arrow function with typed params, got {}: {:?}",
        out.len(),
        out.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Verify that closures capturing outer variables via `use()` resolve
/// correctly during the diagnostic pass.
#[test]
fn closure_use_capture_no_false_positive() {
    let php = r#"<?php
class Logger {
    public string $prefix = '';
    public function log(string $msg): void {}
}

class Worker {
    public function run(): void {
        $logger = new Logger();
        $items = [1, 2, 3];
        array_walk($items, function (int $val) use ($logger): void {
            $logger->log($logger->prefix . ': ' . $val);
        });
    }
}
"#;

    let uri = "file:///test/closure_use.php";
    let backend = create_test_backend_with_full_stubs();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    backend.update_ast(uri, php);

    let mut out = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, php, &mut out);

    assert!(
        out.is_empty(),
        "Expected no diagnostics for closure with use() capture, got {}: {:?}",
        out.len(),
        out.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Verify that closures inside foreach branches with typed parameters
/// resolve correctly during the diagnostic pass (forward walker walks
/// closure bodies inside nested blocks with the correct scope).
#[test]
fn closure_inside_foreach_typed_param_no_false_positive() {
    let php = r#"<?php
class Item {
    public string $name = '';
    public int $price = 0;
}

class Cart {
    /** @param list<Item> $items */
    public function process(array $items): void {
        foreach ($items as $item) {
            // Closure inside foreach body with a typed parameter.
            // The forward walker must find and walk this closure
            // even though it's nested inside a foreach block.
            $fn = function (Item $inner): string {
                return $inner->name;
            };
            echo $item->name;
        }
    }
}
"#;

    let uri = "file:///test/closure_foreach.php";
    let backend = create_test_backend_with_full_stubs();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    backend.update_ast(uri, php);

    let mut out = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, php, &mut out);

    assert!(
        out.is_empty(),
        "Expected no diagnostics for closure inside foreach, got {}: {:?}",
        out.len(),
        out.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Verify that closure parameter shadowing works correctly: a closure
/// parameter that has the same name as an outer variable should resolve
/// to the closure's own parameter type, not the outer type.
#[test]
fn closure_param_shadows_outer_variable_no_false_positive() {
    let php = r#"<?php
class Outer {
    public string $outerProp = '';
}
class Inner {
    public int $innerProp = 0;
}

class ShadowTest {
    public function run(): void {
        $item = new Outer();
        $item->outerProp;

        // The closure parameter $item shadows the outer $item.
        // Inside the closure, $item should be Inner, not Outer.
        $fn = function (Inner $item): void {
            $item->innerProp;
        };

        // After the closure, $item should still be Outer.
        $item->outerProp;
    }
}
"#;

    let uri = "file:///test/closure_shadow.php";
    let backend = create_test_backend_with_full_stubs();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    backend.update_ast(uri, php);

    let mut out = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, php, &mut out);

    assert!(
        out.is_empty(),
        "Expected no diagnostics for closure param shadow, got {}: {:?}",
        out.len(),
        out.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// When a generic class like `Repository<T>` has a method accepting a
/// closure whose parameter type references the template param (e.g.
/// `callable(T): bool`), the forward walker must substitute `T` with
/// the concrete generic arg from the receiver's type.  Without template
/// substitution the closure parameter would be seeded with the raw
/// template name `T`, which doesn't resolve to a class, and member
/// access on it would produce a false positive.
#[test]
fn closure_generic_receiver_template_substitution_no_false_positive() {
    let php = r#"<?php
class Product {
    public string $name = '';
    public int $price = 0;
}

/**
 * @template T
 */
class Repository {
    /**
     * @param callable(T): bool $predicate
     * @return T|null
     */
    public function findFirst(callable $predicate): mixed {
        return null;
    }

    /**
     * @param callable(T): void $callback
     */
    public function each(callable $callback): void {}
}

class ProductService {
    /** @var Repository<Product> */
    private Repository $repo;

    public function demo(): void {
        // The closure parameter $p should resolve to Product via
        // template substitution: Repository<Product> + callable(T) => callable(Product).
        $this->repo->each(function (Product $p): void {
            $p->name;
            $p->price;
        });

        // Arrow function variant.
        $this->repo->findFirst(fn(Product $p) => $p->price > 100);
    }
}
"#;

    let uri = "file:///test/closure_template_sub.php";
    let backend = create_test_backend_with_full_stubs();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    backend.update_ast(uri, php);

    let mut out = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, php, &mut out);

    assert!(
        out.is_empty(),
        "Expected no diagnostics for closure with generic receiver template substitution, got {}: {:?}",
        out.len(),
        out.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// When a closure parameter has no explicit type hint and the callable
/// signature from a generic receiver provides the type via template
/// substitution, the forward walker should infer the parameter type
/// from the substituted callable signature.
#[test]
fn closure_untyped_param_inferred_from_generic_callable_no_false_positive() {
    let php = r#"<?php
class Item {
    public string $label = '';
}

/**
 * @template TItem
 */
class Collection {
    /**
     * @param callable(TItem): void $fn
     */
    public function each(callable $fn): void {}
}

class Demo {
    /** @var Collection<Item> */
    private Collection $items;

    public function run(): void {
        // $item has no explicit type hint — the forward walker infers
        // it from the callable signature after template substitution:
        // Collection<Item> + callable(TItem) => callable(Item).
        $this->items->each(function ($item): void {
            $item->label;
        });
    }
}
"#;

    let uri = "file:///test/closure_untyped_template.php";
    let backend = create_test_backend_with_full_stubs();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    backend.update_ast(uri, php);

    let mut out = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, php, &mut out);

    assert!(
        out.is_empty(),
        "Expected no diagnostics for untyped closure param inferred from generic callable, got {}: {:?}",
        out.len(),
        out.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// When a method chain passes through `static` returns on a generic class,
/// the `ResolvedType::type_string` must carry the reconstructed generic
/// args (e.g. `Builder<Customer>` instead of bare `static`).  Without
/// this, `build_receiver_template_subs` sees no generic args and cannot
/// substitute template parameters in callable params, causing false
/// positives on member accesses inside the closure body.
///
/// Pattern: `Model::where(…)->orderBy(…)->each(fn($items) { $items->first()->… })`
#[test]
fn static_return_chain_preserves_generic_type_string_no_false_positive() {
    let php = r#"<?php
class Customer {
    public string $email = '';
    public function isActive(): bool { return true; }
}

/**
 * @template TKey of array-key
 * @template TValue
 */
class Collection {
    /** @return TValue|null */
    public function first(): mixed { return null; }
    /** @return int */
    public function count(): int { return 0; }
}

/**
 * @template TModel
 */
class Builder {
    /**
     * @param callable(Collection<int, TModel>): mixed $callback
     * @return bool
     */
    public function each(callable $callback): bool { return true; }
    /** @return static */
    public function where(string $col, mixed $val = null): static { return $this; }
    /** @return static */
    public function orderBy(string $col): static { return $this; }
}

class CustomerQuery {
    /** @return Builder<Customer> */
    public static function where(string $col, mixed $val = null): Builder { return new Builder(); }
}

class Service {
    public function run(): void {
        // Chain: CustomerQuery::where() => Builder<Customer>
        //        ->orderBy() => static (must reconstruct to Builder<Customer>)
        //        ->each(fn($items)) => $items is Collection<int, Customer>
        CustomerQuery::where('active', true)->orderBy('name')->each(function ($items) {
            $items->count();
            $items->first();
        });
    }
}
"#;

    let uri = "file:///test/static_chain_generic.php";
    let backend = create_test_backend_with_full_stubs();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    backend.update_ast(uri, php);

    let mut out = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, php, &mut out);

    assert!(
        out.is_empty(),
        "Expected no diagnostics for closure param after static return chain, got {}: {:?}",
        out.len(),
        out.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Same pattern but with multiple chained `static` returns and a foreach
/// inside the closure body.  Verifies generic args propagate through
/// several hops of `static` and into iteration.
#[test]
fn static_return_chain_foreach_inside_closure_no_false_positive() {
    let php = r#"<?php
class User {
    public string $email = '';
    public function sendWelcome(): void {}
}

/**
 * @template TKey of array-key
 * @template TValue
 * @implements IteratorAggregate<TKey, TValue>
 */
class Collection implements IteratorAggregate {
    /** @return TValue|null */
    public function first(): mixed { return null; }
    /** @return Traversable<TKey, TValue> */
    public function getIterator(): Traversable { return new ArrayIterator([]); }
}

/**
 * @template TModel
 */
class Builder {
    /**
     * @param callable(Collection<int, TModel>): mixed $callback
     * @return bool
     */
    public function each(callable $callback): bool { return true; }
    /** @return static */
    public function where(string $col, mixed $val = null): static { return $this; }
    /** @return static */
    public function orderBy(string $col): static { return $this; }
    /** @return static */
    public function limit(int $n): static { return $this; }
}

class UserQuery {
    /** @return Builder<User> */
    public static function where(string $col, mixed $val = null): Builder { return new Builder(); }
}

class Mailer {
    public function sendAll(): void {
        UserQuery::where('active', true)->orderBy('name')->limit(50)->each(function ($users) {
            foreach ($users as $user) {
                $user->sendWelcome();
                $user->email;
            }
        });
    }
}
"#;

    let uri = "file:///test/static_chain_foreach.php";
    let backend = create_test_backend_with_full_stubs();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    backend.update_ast(uri, php);

    let mut out = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, php, &mut out);

    assert!(
        out.is_empty(),
        "Expected no diagnostics for foreach inside closure after static return chain, got {}: {:?}",
        out.len(),
        out.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// When the callable signature from a generic receiver contains unresolved
/// template parameters (e.g. `callable(Collection<TModel>)` where `TModel`
/// wasn't substituted because `build_receiver_template_subs` couldn't
/// reconstruct the generic args), but the closure parameter has an
/// explicit type hint, the forward walker should use the explicit hint
/// and walk the closure body normally instead of bailing out to the
/// backward scanner.
///
/// This tests Step 11 progress: the forward walker filters out inferred
/// types with unresolved template names during `seed_closure_params`,
/// preventing the `scope_has_unresolved_template_types` guard from
/// triggering unnecessarily.
#[test]
fn closure_explicit_hint_with_unresolved_template_inferred_no_false_positive() {
    // Verify that callable param extraction preserves generic
    // substitutions from the receiver's already-specialized class_info.
    //
    // Setup:
    // - Stream<TKey, TVal> has TWO template params
    // - Stream has no `extends_generics` and no self-referencing
    //   method return types (so `build_receiver_template_subs` cannot
    //   reconstruct generic args from the type_string)
    // - Methods return `static` (type_string degrades to bare "static")
    //
    // Before Step 11: `find_callable_params_on_classes_fw` called
    // `resolve_class_fully_maybe_cached` which re-resolved from the
    // base FQN with empty generic args, discarding substitutions.
    // The callable param type was `callable(Collection<TVal>)` with
    // TVal unsubstituted, triggering the template guard and forcing
    // fallback to the backward scanner.
    //
    // After Step 11: callable params are extracted from the input
    // class_info first (which already has {TVal → Product} applied),
    // producing `callable(Collection<Product>)`.  The closure
    // parameter `Collection $items` gets the inferred
    // `Collection<Product>` (more specific than the bare hint),
    // and the forward walker walks the body with full generic context.
    let php = r#"<?php
class Product {
    public string $name = '';
    public int $price = 0;
    public function discount(): float { return 0.0; }
}

/**
 * @template TKey of array-key
 * @template TValue
 */
class Collection {
    /** @return TValue|null */
    public function first(): mixed { return null; }
}

/**
 * @template TKey of array-key
 * @template TVal
 */
class Stream {
    /** @return static */
    public function filter(callable $predicate): static { return $this; }
    /** @return static */
    public function sort(string $col): static { return $this; }

    /**
     * @param callable(Collection<TVal>): void $fn
     */
    public function each(callable $fn): void {}

    /** @return int */
    public function count(): int { return 0; }
}

class StreamFactory {
    /** @return Stream<int, Product> */
    public static function products(): Stream { return new Stream(); }
}

class Consumer {
    public function run(): void {
        // StreamFactory::products() returns Stream<int, Product>.
        // ->filter(...)->sort('name') chains through `static` returns,
        // degrading the type_string to bare "static".
        // The input class_info still carries {TVal → Product}, so
        // callable params are extracted as callable(Collection<Product>).
        // The closure's explicit Collection hint gets upgraded to
        // Collection<Product> via inferred_type_is_more_specific.
        StreamFactory::products()->filter(fn($x) => true)->sort('name')->each(function (Collection $items): void {
            $order = new Product();
            $order->name;
            $order->price;
            $order->discount();
        });
    }
}
"#;

    let uri = "file:///test/closure_explicit_hint_unresolved_template.php";
    let backend = create_test_backend_with_full_stubs();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    backend.update_ast(uri, php);

    let mut out = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, php, &mut out);

    assert!(
        out.is_empty(),
        "Expected no diagnostics when closure has explicit type hint despite unresolved template in inferred type, got {}: {:?}",
        out.len(),
        out.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Verify that `$this` resolves correctly from the forward walker's scope
/// cache in non-static class methods.
///
/// Before this fix, the forward walker did not seed `$this` in the scope,
/// so every `$this->prop` or `foreach ($this->items() as $item)` expression
/// inside the forward walk fell through to the backward scanner.  Now
/// `$this` is seeded as a `ResolvedType` from the enclosing `ClassInfo`,
/// eliminating the fallthrough.
#[test]
fn this_resolves_from_scope_cache_in_method_body() {
    let php = r#"<?php
class Pet {
    public string $name = '';
    public function speak(): string { return ''; }
}

class Owner {
    /** @var Pet[] */
    private array $pets = [];

    public function pet(): Pet { return new Pet(); }

    public function run(): void {
        // Direct $this property/method access.
        $p = $this->pet();
        $p->name;
        $p->speak();

        // $this inside foreach iterable expression.
        foreach ($this->pets as $pet) {
            $pet->name;
            $pet->speak();
        }
    }
}
"#;

    let uri = "file:///test/this_scope_cache.php";
    let backend = create_test_backend_with_full_stubs();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    backend.update_ast(uri, php);

    let mut out = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, php, &mut out);

    assert!(
        out.is_empty(),
        "Expected no diagnostics for $this member access in method body, got {}: {:?}",
        out.len(),
        out.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Verify that `$this` is implicitly captured inside closures (PHP behaviour)
/// and resolves correctly from the forward walker's scope cache.
#[test]
fn this_resolves_inside_closure_from_scope_cache() {
    let php = r#"<?php
class Item {
    public string $label = '';
}

class Service {
    /** @return Item[] */
    public function items(): array { return []; }

    public function run(): void {
        $fn = function (): void {
            // $this is implicitly available inside closures in PHP.
            foreach ($this->items() as $item) {
                $item->label;
            }
        };
    }

    public function runWithUse(): void {
        $extra = new Item();
        $fn = function () use ($extra): void {
            $extra->label;
            foreach ($this->items() as $item) {
                $item->label;
            }
        };
    }
}
"#;

    let uri = "file:///test/this_closure_scope_cache.php";
    let backend = create_test_backend_with_full_stubs();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    backend.update_ast(uri, php);

    let mut out = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, php, &mut out);

    assert!(
        out.is_empty(),
        "Expected no diagnostics for $this inside closures, got {}: {:?}",
        out.len(),
        out.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Verify that `$this` is NOT seeded in static methods.
#[test]
fn this_not_seeded_in_static_method() {
    // In a static method, `$this` is not available.  The forward walker
    // should not seed it, so `$this->method()` should not resolve.
    // We test indirectly: a variable assigned from `$this->method()`
    // should not resolve (producing a diagnostic on member access).
    let php = r#"<?php
class Widget {
    public string $title = '';
}

class Factory {
    public function widget(): Widget { return new Widget(); }

    public static function build(): void {
        // $this is not available in static methods.  The scope cache
        // should not have an entry, so $w will not resolve and
        // $w->title will produce a diagnostic.
        $w = $this->widget();
        $w->title;
    }
}
"#;

    let uri = "file:///test/this_static_no_seed.php";
    let backend = create_test_backend_with_full_stubs();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    backend.update_ast(uri, php);

    let mut out = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, php, &mut out);

    assert!(
        !out.is_empty(),
        "Expected at least one diagnostic: $this should not resolve in a static method, \
         so $w->title should flag as unknown member access"
    );
}

/// When a closure parameter has no explicit type hint AND the inferred
/// type from the callable signature contains an unresolved template
/// parameter, the forward walker should skip the unresolvable inferred
/// type (leaving the param untyped) and still walk the closure body.
/// Variables assigned inside the closure body from other sources should
/// still resolve correctly.
#[test]
fn closure_untyped_param_with_unresolved_template_walks_body() {
    // Same multi-template-param Stream scenario, but the closure
    // parameter has NO explicit type hint.  Before Step 11, the
    // inferred type was bare `TVal` (unresolved) because
    // `resolve_class_fully_maybe_cached` discarded substitutions.
    // After Step 11, the inferred type is `Order` (the concrete
    // substitution for TVal), so $item resolves correctly AND the
    // closure body is walked by the forward walker.
    let php = r#"<?php
class Order {
    public string $number = '';
}

/**
 * @template TKey of array-key
 * @template TVal
 */
class Stream {
    /** @return static */
    public function filter(callable $predicate): static { return $this; }

    /**
     * @param callable(TVal): void $fn
     */
    public function each(callable $fn): void {}
}

class StreamFactory {
    /** @return Stream<int, Order> */
    public static function orders(): Stream { return new Stream(); }
}

class OrderReport {
    public function generate(): void {
        // The chain degrades the type_string to bare "static", but
        // the input class_info carries {TVal → Order}.  Callable
        // param extraction uses the input class first, producing
        // callable(Order).  $item gets inferred as Order, and the
        // forward walker walks the closure body with correct types.
        StreamFactory::orders()->filter(fn($x) => true)->each(function ($item): void {
            // $item should resolve to Order via the substituted callable param.
            $item->number;
            $order = new Order();
            $order->number;
        });
    }
}
"#;

    let uri = "file:///test/closure_untyped_unresolved_template.php";
    let backend = create_test_backend_with_full_stubs();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    backend.update_ast(uri, php);

    let mut out = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, php, &mut out);

    assert!(
        out.is_empty(),
        "Expected no diagnostics for variables assigned inside closure body when param has unresolved template, got {}: {:?}",
        out.len(),
        out.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Cross-file warm-cache test with a real PSR-4 workspace.
///
/// This is the scenario that actually benefits from targeted invalidation:
/// vendor/framework classes live in separate files, the user edits only
/// their own file.  On the warm run, all vendor class resolutions stay
/// cached because `update_ast` only evicts FQNs defined in the edited file.
///
/// `example.php` puts everything in one file, so ALL FQNs get evicted on
/// every edit and the cache provides no cross-edit benefit.  This test
/// shows the real-world improvement.
#[tokio::test]
async fn time_diagnostics_warm_cache_cross_file() {
    let composer_json = r#"{
        "autoload": {
            "psr-4": {
                "App\\": "src/",
                "Illuminate\\Database\\Eloquent\\": "vendor/illuminate/Eloquent/",
                "Illuminate\\Database\\Query\\": "vendor/illuminate/Query/",
                "Illuminate\\Database\\Concerns\\": "vendor/illuminate/Concerns/",
                "Illuminate\\Support\\": "vendor/illuminate/Support/"
            }
        }
    }"#;

    let model_php = r#"<?php
namespace Illuminate\Database\Eloquent;
/**
 * @method static \Illuminate\Database\Eloquent\Builder<static> where(string $column, mixed $operator = null, mixed $value = null)
 * @method static \Illuminate\Database\Eloquent\Builder<static> query()
 */
abstract class Model {
    /** @deprecated Use newQuery() instead */
    public static function on(string $connection = null): Builder { return new Builder(); }
}
"#;

    let builder_php = r#"<?php
namespace Illuminate\Database\Eloquent;
use Illuminate\Database\Concerns\BuildsQueries;
/**
 * @template TModel of \Illuminate\Database\Eloquent\Model
 * @mixin \Illuminate\Database\Query\Builder
 */
class Builder {
    /** @use BuildsQueries<TModel> */
    use BuildsQueries;
    /** @return static */
    public function where(string $column, mixed $operator = null, mixed $value = null): static { return $this; }
    /** @return static */
    public function orderBy(string $column, string $direction = 'asc'): static { return $this; }
    /** @return \Illuminate\Database\Eloquent\Collection<int, TModel> */
    public function get(): Collection { return new Collection(); }
    /** @return static */
    public function limit(int $value): static { return $this; }
    /** @deprecated Use where() instead */
    public function whereRaw(string $sql): static { return $this; }
}
"#;

    let query_builder_php = r#"<?php
namespace Illuminate\Database\Query;
class Builder {
    /** @return static */
    public function whereIn(string $column, array $values): static { return $this; }
    /** @return static */
    public function groupBy(string ...$groups): static { return $this; }
    /** @deprecated Use whereIn() instead */
    public function whereInRaw(string $column, array $values): static { return $this; }
}
"#;

    let builds_queries_php = r#"<?php
namespace Illuminate\Database\Concerns;
/** @template TValue */
trait BuildsQueries {
    /** @return TValue|null */
    public function first(): mixed { return null; }
}
"#;

    let collection_php = r#"<?php
namespace Illuminate\Database\Eloquent;
/**
 * @template TKey of array-key
 * @template TModel
 */
class Collection {
    /** @return TModel|null */
    public function first(): mixed { return null; }
    /** @deprecated Use firstOrFail() instead */
    public function firstOr(): mixed { return null; }
}
"#;

    let support_collection_php = r#"<?php
namespace Illuminate\Support;
/**
 * @template TKey of array-key
 * @template TValue
 */
class Collection {
    /** @return TValue|null */
    public function first(): mixed { return null; }
    /** @return static */
    public function filter(callable $callback = null): static { return $this; }
}
"#;

    // User file: exercises many vendor class references.
    // Deliberately large to stress the resolved-class cache.  Each
    // `->method()` call triggers a `resolve_class_fully_cached` lookup,
    // so 50+ member accesses on vendor classes shows a clear speedup
    // when those resolutions survive across edits.
    let user_php = r#"<?php
namespace App;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Builder;
use Illuminate\Database\Eloquent\Collection;

class Brand extends Model {
    public function scopeActive(Builder $query): void {}
    public function scopeOfGenre(Builder $query, string $genre): void {}
}

class Product extends Model {
    public function scopeInStock(Builder $query): void {}
}

class Category extends Model {
    public function scopeVisible(Builder $query): void {}
}

class UserService {
    public function brands(): void {
        $q1 = Brand::where('active', true);
        $q1->orderBy('name')->get();
        $q1->active();
        $q1->ofGenre('fiction');
        $q1->limit(10)->get();
        $q1->orderBy('created_at')->limit(5)->get();

        Brand::where('genre', 'fiction')->ofGenre('sci-fi')->get();
        Brand::where('active', 1)->orderBy('name')->first();
        Brand::where('active', 1)->orderBy('name')->limit(5)->get();
        Brand::where('x', 1)->where('y', 2)->where('z', 3)->get();
        Brand::where('a', 1)->active()->ofGenre('x')->orderBy('name')->get();
        Brand::where('b', 1)->limit(1)->first();
        Brand::where('c', 1)->get()->first();
    }

    public function products(): void {
        Product::where('in_stock', true)->inStock()->get();
        Product::where('price', '>', 100)->limit(10)->get();
        Product::where('active', true)->orderBy('price')->get();
        Product::where('active', true)->orderBy('price')->limit(20)->get();
        Product::where('active', true)->orderBy('name')->first();
        Product::where('active', true)->inStock()->orderBy('name')->get();
        Product::where('x', 1)->where('y', 2)->get();
        Product::where('x', 1)->where('y', 2)->limit(5)->get();
        Product::where('x', 1)->where('y', 2)->first();
        Product::where('x', 1)->where('y', 2)->orderBy('z')->get();

        $p = Product::where('active', true)->get();
        $p->first();
        $p2 = Product::where('price', '>', 50)->get();
        $p2->first();
        $p3 = Product::where('stock', '>', 0)->get();
        $p3->first();
    }

    public function categories(): void {
        Category::where('active', true)->visible()->get();
        Category::where('active', true)->orderBy('name')->get();
        Category::where('active', true)->orderBy('name')->limit(5)->get();
        Category::where('active', true)->orderBy('name')->first();
        Category::where('x', 1)->where('y', 2)->visible()->get();
        Category::where('x', 1)->where('y', 2)->orderBy('z')->get();
        Category::where('x', 1)->where('y', 2)->limit(10)->get();
        Category::where('x', 1)->visible()->orderBy('name')->limit(5)->get();

        $c = Category::where('active', true)->get();
        $c->first();
        $c2 = Category::where('parent_id', null)->get();
        $c2->first();
    }

    public function mixed(): void {
        $brands = Brand::where('active', true)->get();
        $brands->first();
        $products = Product::where('active', true)->get();
        $products->first();
        $categories = Category::where('active', true)->get();
        $categories->first();

        Brand::where('a', 1)->orderBy('b')->limit(5)->get()->first();
        Product::where('a', 1)->orderBy('b')->limit(5)->get()->first();
        Category::where('a', 1)->orderBy('b')->limit(5)->get()->first();

        Brand::where('x', 1)->get();
        Brand::where('x', 2)->get();
        Brand::where('x', 3)->get();
        Product::where('x', 1)->get();
        Product::where('x', 2)->get();
        Product::where('x', 3)->get();
        Category::where('x', 1)->get();
        Category::where('x', 2)->get();
        Category::where('x', 3)->get();
    }
}
"#;

    let (backend, _dir) = create_psr4_workspace(
        composer_json,
        &[
            ("vendor/illuminate/Eloquent/Model.php", model_php),
            ("vendor/illuminate/Eloquent/Builder.php", builder_php),
            ("vendor/illuminate/Eloquent/Collection.php", collection_php),
            ("vendor/illuminate/Query/Builder.php", query_builder_php),
            (
                "vendor/illuminate/Concerns/BuildsQueries.php",
                builds_queries_php,
            ),
            (
                "vendor/illuminate/Support/Collection.php",
                support_collection_php,
            ),
            ("src/UserService.php", user_php),
        ],
    );

    let user_uri = format!(
        "file://{}",
        _dir.path().join("src/UserService.php").display()
    );
    backend.update_ast(&user_uri, user_php);

    // ── Cold run: populates the resolved-class cache ────────────────────
    let start_cold = Instant::now();
    let mut out_cold = Vec::new();
    backend.collect_deprecated_diagnostics(&user_uri, user_php, &mut out_cold);
    backend.collect_unused_import_diagnostics(&user_uri, user_php, &mut out_cold);
    backend.collect_unknown_class_diagnostics(&user_uri, user_php, &mut out_cold);
    let cold_total = start_cold.elapsed();
    let cold_count = out_cold.len();

    // ── Simulate editing the user file only ─────────────────────────────
    // This evicts App\Brand, App\Product, App\UserService from the cache
    // but leaves all Illuminate\* entries intact.
    backend.update_ast(&user_uri, user_php);

    // ── Warm run: vendor classes are still cached ───────────────────────
    let start_warm = Instant::now();
    let mut out_warm = Vec::new();
    backend.collect_deprecated_diagnostics(&user_uri, user_php, &mut out_warm);
    backend.collect_unused_import_diagnostics(&user_uri, user_php, &mut out_warm);
    backend.collect_unknown_class_diagnostics(&user_uri, user_php, &mut out_warm);
    let warm_total = start_warm.elapsed();
    let warm_count = out_warm.len();

    eprintln!();
    eprintln!("=== Cross-file warm-cache diagnostic timing ===");
    eprintln!(
        "  cold run:  {:>10.3?}  ({} diagnostics)",
        cold_total, cold_count
    );
    eprintln!(
        "  warm run:  {:>10.3?}  ({} diagnostics)",
        warm_total, warm_count
    );
    let speedup = cold_total.as_secs_f64() / warm_total.as_secs_f64().max(0.000001);
    eprintln!("  speedup:   {:.1}x", speedup);
    eprintln!();

    assert_eq!(
        cold_count, warm_count,
        "warm run produced different diagnostic count ({} vs {})",
        warm_count, cold_count
    );
}

#[tokio::test]
#[ignore] // benchmark — takes ~12 s; run with --run-ignored all
async fn time_diagnostics_on_phpstan_fixture() {
    let path = "benches/fixtures/diagnostics/phpstan.php";
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("Skipping: {path} not found");
            return;
        }
    };
    let uri = "file:///bench/phpstan.php";
    let backend = create_test_backend_with_full_stubs();
    backend.update_ast(uri, &content);

    let start = Instant::now();
    let mut deprecated_out = Vec::new();
    backend.collect_deprecated_diagnostics(uri, &content, &mut deprecated_out);
    let deprecated_time = start.elapsed();

    let start = Instant::now();
    let mut unused_out = Vec::new();
    backend.collect_unused_import_diagnostics(uri, &content, &mut unused_out);
    let unused_time = start.elapsed();

    let start = Instant::now();
    let mut unknown_out = Vec::new();
    backend.collect_unknown_class_diagnostics(uri, &content, &mut unknown_out);
    let unknown_time = start.elapsed();

    let total = deprecated_time + unused_time + unknown_time;

    eprintln!();
    eprintln!(
        "=== Diagnostic timing on phpstan.php ({} lines) ===",
        content.lines().count()
    );
    eprintln!(
        "  deprecated:     {:>10.3?}  ({} diagnostics)",
        deprecated_time,
        deprecated_out.len()
    );
    eprintln!(
        "  unused_imports: {:>10.3?}  ({} diagnostics)",
        unused_time,
        unused_out.len()
    );
    eprintln!(
        "  unknown_classes:{:>10.3?}  ({} diagnostics)",
        unknown_time,
        unknown_out.len()
    );
    eprintln!("  ──────────────────────────────────");
    eprintln!("  TOTAL:          {:>10.3?}", total);
    eprintln!();

    let budget_secs = if cfg!(debug_assertions) { 120.0 } else { 5.0 };
    assert!(
        total.as_secs_f64() < budget_secs,
        "Diagnostics took {:.3?} on the large phpstan fixture — too slow for interactive use \
         (budget: {:.0} s).",
        total,
        budget_secs,
    );
}

/// Warm-cache test on the phpstan fixture (larger file, more class references).
#[tokio::test]
#[ignore] // benchmark — takes ~21 s; run with --run-ignored all
async fn time_diagnostics_warm_cache_phpstan() {
    let path = "benches/fixtures/diagnostics/phpstan.php";
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("Skipping: {path} not found");
            return;
        }
    };
    let uri = "file:///bench/phpstan.php";
    let backend = create_test_backend_with_full_stubs();
    backend.update_ast(uri, &content);

    // ── Cold run ────────────────────────────────────────────────────────
    let start_cold = Instant::now();
    let mut out = Vec::new();
    backend.collect_deprecated_diagnostics(uri, &content, &mut out);
    backend.collect_unused_import_diagnostics(uri, &content, &mut out);
    backend.collect_unknown_class_diagnostics(uri, &content, &mut out);
    let cold_total = start_cold.elapsed();
    let cold_count = out.len();

    // ── Simulate edit ───────────────────────────────────────────────────
    backend.update_ast(uri, &content);

    // ── Warm run ────────────────────────────────────────────────────────
    let start_warm = Instant::now();
    let mut out_warm = Vec::new();
    backend.collect_deprecated_diagnostics(uri, &content, &mut out_warm);
    backend.collect_unused_import_diagnostics(uri, &content, &mut out_warm);
    backend.collect_unknown_class_diagnostics(uri, &content, &mut out_warm);
    let warm_total = start_warm.elapsed();
    let warm_count = out_warm.len();

    eprintln!();
    eprintln!(
        "=== Warm-cache diagnostic timing on phpstan.php ({} lines) ===",
        content.lines().count()
    );
    eprintln!(
        "  cold run:  {:>10.3?}  ({} diagnostics)",
        cold_total, cold_count
    );
    eprintln!(
        "  warm run:  {:>10.3?}  ({} diagnostics)",
        warm_total, warm_count
    );
    let speedup = cold_total.as_secs_f64() / warm_total.as_secs_f64().max(0.000001);
    eprintln!("  speedup:   {:.1}x", speedup);
    eprintln!();

    assert_eq!(
        cold_count, warm_count,
        "warm run produced different diagnostic count ({} vs {})",
        warm_count, cold_count
    );
}

/// Fluent method chains on classes whose methods return `self` (e.g.
/// `Decimal::sub()->div()`) must resolve through the entire chain.
/// The forward walker stores `$var` as the class type; the diagnostic
/// collector then resolves `$var->sub($x)` to the same class (via the
/// `self` return type) and verifies that `->div()` exists on it.
///
/// Regression: the scope cache stored the variable correctly, but the
/// intermediate chain `$var->sub($x)` was not resolved because the
/// subject resolution pipeline lost the type mid-chain.
#[test]
fn self_returning_method_chain_no_false_positive() {
    let php = r#"<?php
class Decimal {
    public function add(int|self|string $value): self { return $this; }
    public function sub(int|self|string $value): self { return $this; }
    public function mul(int|self|string $value): self { return $this; }
    public function div(int|self|string $value): self { return $this; }
    public function isZero(): bool { return true; }
    public function toInt(): int { return 0; }
}

class Calculator {
    public function compute(Decimal $net, Decimal $supplierPrice): Decimal {
        $denominator = $net->mul(100);
        if ($denominator->isZero()) {
            return new Decimal();
        }
        return $denominator->sub($supplierPrice)->div($denominator);
    }

    public static function staticCompute(Decimal $a, Decimal $b): int {
        $result = $a->mul(100)->div($b->add(100));
        if ($result->isZero()) {
            return 0;
        }
        return $a->sub($b)->div($a)->mul(100)->toInt();
    }
}
"#;

    let uri = "file:///test/decimal_chain.php";
    let backend = create_test_backend_with_full_stubs();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    backend.update_ast(uri, php);

    let mut out = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, php, &mut out);

    assert!(
        out.is_empty(),
        "Expected no diagnostics for self-returning method chains, got {}: {:?}",
        out.len(),
        out.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Cross-file variant: when `Decimal` lives in a separate PSR-4 file,
/// the method chain `$var->sub($x)->div($y)` must still resolve through
/// the `self` return type.  This is the pattern that caused false
/// positives in the real codebase (`Monetary.php`, `ProductRedLineService.php`,
/// `ProductTranslation.php`).
#[test]
fn self_returning_method_chain_cross_file_no_false_positive() {
    let decimal_php = r#"<?php
namespace App\Decimal;

class Decimal {
    public function add(int|self|string $value): self { return $this; }
    public function sub(int|self|string $value): self { return $this; }
    public function mul(int|self|string $value): self { return $this; }
    public function div(int|self|string $value): self { return $this; }
    public function isZero(): bool { return true; }
    public function toInt(): int { return 0; }
}
"#;

    let calculator_php = r#"<?php
namespace App\Calculator;

use App\Decimal\Decimal;

class Calculator {
    public function compute(Decimal $net, Decimal $supplierPrice): Decimal {
        $denominator = $net->mul(100);
        if ($denominator->isZero()) {
            return new Decimal();
        }
        return $denominator->sub($supplierPrice)->div($denominator);
    }

    public static function staticCompute(Decimal $a, Decimal $b): int {
        $result = $a->mul(100)->div($b->add(100));
        if ($result->isZero()) {
            return 0;
        }
        return $a->sub($b)->div($a)->mul(100)->toInt();
    }
}
"#;

    let composer = r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#;
    let (backend, _dir) = create_psr4_workspace(
        composer,
        &[
            ("src/Decimal/Decimal.php", decimal_php),
            ("src/Calculator/Calculator.php", calculator_php),
        ],
    );
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }

    let uri = "file:///test/src/Calculator/Calculator.php";
    backend.update_ast(uri, calculator_php);

    let mut out = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, calculator_php, &mut out);

    assert!(
        out.is_empty(),
        "Expected no diagnostics for cross-file self-returning method chains, got {}: {:?}",
        out.len(),
        out.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ─── Foreach over array of ::class literals resolves static access ───────────

#[test]
fn foreach_class_string_array_static_access_no_false_positive() {
    let php = r#"<?php
class Page {
    public const TABLE_NAME = 'pages';
}

class Newsletter {
    public const TABLE_NAME = 'newsletters';
}

class Controller {
    public function test(): void {
        foreach ([Page::class, Newsletter::class] as $className) {
            echo $className::TABLE_NAME;
        }
    }
}
"#;

    let uri = "file:///test/foreach_class.php";
    let backend = create_test_backend();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    backend.update_ast(uri, php);

    let mut out = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, php, &mut out);

    let relevant: Vec<_> = out
        .iter()
        .filter(|d| {
            d.code.as_ref().is_some_and(|c| {
                matches!(c, NumberOrString::String(s) if s == "unknown_member" || s == "unresolved_member_access")
            })
        })
        .collect();
    assert!(
        relevant.is_empty(),
        "Should not flag static access on foreach class-string variable, got: {:?}",
        relevant.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}
