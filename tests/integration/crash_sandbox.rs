use crate::common::create_test_backend;
use tower_lsp::lsp_types::Position;

/// Helper: send a hover request at (line, character) and return the result.
fn hover_at(
    backend: &phpantom_lsp::Backend,
    uri: &str,
    content: &str,
    line: u32,
    character: u32,
) -> Option<tower_lsp::lsp_types::Hover> {
    backend.update_ast(uri, content);
    backend.handle_hover(uri, content, Position { line, character })
}

/// Regression test: parse the exact sandbox.php content through
/// update_ast (the did_open code path) to verify it does not crash
/// the parser or symbol-map extraction.
#[test]
fn sandbox_exact_content_update_ast_does_not_crash() {
    let backend = create_test_backend();
    let uri = "file:///sandbox.php";

    // This is the exact content from the bug report.
    let content = r#"<?php

namespace App\Http\Controllers\Economy;

use App\Http\Controllers\Controller;
use Vendor\Shared\Common\Convert;
use Vendor\Shared\Common\ConvertException;
use Exception;
use Illuminate\Contracts\View\View;
use Illuminate\Database\Query\Builder;
use Illuminate\Support\Facades\DB;
use Acme\Core\Enums\Country;
use Acme\Core\Enums\OrderStatus;
use Acme\Decimal\Decimal;
use stdClass;

final class ExtractionToolController extends Controller
{
    public function extractionToolIndex(): View
    {
        $this->getAdmin()->verifyPermissions('economy.extraction_json');

        return view('economy.extractiontool');
    }

    /**
     * @throws ConvertException
     * @throws Exception
     *
     * @return array<string, mixed>
     */
    public function extraction_json(string $from_date, string $to_date, Country $from_site): array
    {
        $this->getAdmin()->verifyPermissions('economy.extraction_json');

        $subscriptionGateways = DB::table('subscriptions')
            ->select(DB::raw('gateway'))
            ->leftJoin('users', 'users.id', '=', 'subscriptions.user_id')
            ->where('users.country', $from_site)
            ->where('subscriptions.user_id', '>', 0)
            ->where('subscriptions.created', '>=', $from_date)
            ->where('subscriptions.created', '<=', $to_date)
            ->groupBy('gateway')->pluck('gateway');
        $numbers = [
            'sub_price'             => new Decimal(0),
            'sub_price_without_vat' => new Decimal(0),
        ];
        foreach ($subscriptionGateways as $gateway) {
            $tmpNumbers = DB::table('subscriptions')
                ->select(DB::raw('
                    SUM(subscriptions.price) AS sub_price,
                    (SUM(subscriptions.price) * (100/(100+vat_percentage))) AS sub_price_without_vat'))
                ->join('users', 'users.id', '=', 'subscriptions.user_id')
                ->where('users.country', $from_site)
                ->where('gateway', $gateway)
                ->where('subscriptions.user_id', '>', 0)
                ->where('subscriptions.created', '>=', $from_date)
                ->where('subscriptions.created', '<=', $to_date)
                ->where(function (Builder $query): void {
                    $query->whereNull('is_paid')
                        ->orWhere('is_paid', 1);
                })
                ->first();
            if (!$tmpNumbers instanceof stdClass) {
                throw new Exception('Subscription numbers not found');
            }
            $numbers['sub_price'] = $numbers['sub_price']->add(Convert::toDecimal($tmpNumbers->sub_price));
            $numbers['sub_price_without_vat'] = $numbers['sub_price_without_vat']->add(Convert::toDecimal($tmpNumbers->sub_price_without_vat));
        }

        $orders = DB::table('orders')
            ->select(DB::raw('
                SUM(amount) AS amount,
                SUM(postage) AS postage,
                SUM(subscription) AS subscription,
                SUM((orders.amount - orders.postage - ifnull(orders.subscription,0)) * (100/(100+vat_percentage))) as product_sales_without_vat'))
            ->where('status', OrderStatus::STATUS_DELIVERED)
            ->where('country', $from_site)
            ->where('created', '>=', $from_date)
            ->where('created', '<=', $to_date)
            ->first();
        if (!$orders instanceof stdClass) {
            throw new Exception('Order numbers not found');
        }

        $cancelled_orders = DB::table('orders')
            ->select(DB::raw('
                SUM(amount) AS amount,
                SUM(postage) AS postage,
                SUM(subscription) AS subscription,
                SUM((orders.amount - orders.postage - ifnull(orders.subscription,0)) * (100/(100+vat_percentage))) as products_without_vat'))
            ->where('status', OrderStatus::STATUS_CANCELLED)
            ->where('country', $from_site)
            ->where('created', '>=', $from_date)
            ->where('created', '<=', $to_date)
            ->first();

        $data = [];
        $data['subscription_sales'] = $numbers['sub_price'];
        $data['postage_sales'] = $orders->postage;
        $data['product_sales'] = Convert::toDecimal($orders->amount)->sub(Convert::toDecimal($orders->postage))->sub(Convert::toDecimal($orders->subscription));
        $data['product_sales_without_vat'] = $orders->product_sales_without_vat;
        $data['subscription_sales_without_vat'] = $numbers['sub_price_without_vat'];

        $data['cancelled_orders'] = $cancelled_orders;

        $data['from_date'] = $from_date;
        $data['to_date'] = $to_date;
        $data['from_site'] = $from_site->value;

        $data['country'] = '';
        $data['currency'] = $from_site->getCurrency()->value;

        return $data;
    }
}
"#;

    // Step 1: update_ast must not crash (parser + symbol map extraction).
    backend.update_ast(uri, content);

    // Step 2: hover at every line in the method body to exercise
    // the full resolution pipeline.  Before the fix, line 66
    // (`$numbers['sub_price'] = $numbers['sub_price']->add(...)`)
    // caused infinite recursion and a stack overflow because the
    // raw-type inference path did not reduce cursor_offset for
    // self-referential array-key assignments.
    let lines: Vec<&str> = content.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let col = line.len().min(20) as u32;
        // This must not stack-overflow or hang.
        let _ = backend.handle_hover(
            uri,
            content,
            Position {
                line: i as u32,
                character: col,
            },
        );
    }
}

/// The sandbox.php file from the bug report causes the LSP to crash (zombie
/// process).  The file features:
///
/// - Very deep method chains on a query builder (8-10 chained calls)
/// - A closure parameter with an explicit type hint passed mid-chain
/// - Self-referential array key access (`$numbers['sub_price']->add(...)`)
/// - Multiple such chains in the same method body
///
/// This test verifies the LSP does not stack-overflow or hang on this pattern.
#[test]
fn sandbox_deep_chain_with_closure_does_not_crash() {
    let backend = create_test_backend();
    let uri = "file:///sandbox.php";

    // Scaffolding: minimal stubs for the classes used in the sandbox file.
    let stub_content = r#"<?php
namespace Illuminate\Database\Query;
class Builder {
    /** @return static */
    public function select(mixed ...$columns): static { return $this; }
    /** @return static */
    public function leftJoin(string $table, string $first, string $operator = null, string $second = null): static { return $this; }
    /** @return static */
    public function join(string $table, string $first, string $operator = null, string $second = null): static { return $this; }
    /** @return static */
    public function where(mixed $column, mixed $operator = null, mixed $value = null): static { return $this; }
    /** @return static */
    public function whereNull(string $column): static { return $this; }
    /** @return static */
    public function orWhere(mixed ...$args): static { return $this; }
    /** @return static */
    public function groupBy(string ...$groups): static { return $this; }
    /** @return \Illuminate\Support\Collection */
    public function pluck(string $column, ?string $key = null): \Illuminate\Support\Collection { }
    /** @return ?\stdClass */
    public function first(): ?\stdClass { }
}
"#;
    backend.update_ast("file:///Builder.php", stub_content);

    let db_stub = r#"<?php
namespace Illuminate\Support\Facades;
class DB {
    /**
     * @return \Illuminate\Database\Query\Builder
     */
    public static function table(string $table): \Illuminate\Database\Query\Builder {}
    /**
     * @return \Illuminate\Database\Eloquent\Expression
     */
    public static function raw(string $value): \Illuminate\Database\Eloquent\Expression {}
}
"#;
    backend.update_ast("file:///DB.php", db_stub);

    let collection_stub = r#"<?php
namespace Illuminate\Support;
class Collection {
    /** @return mixed */
    public function first(): mixed {}
    /** @return array */
    public function toArray(): array {}
}
"#;
    backend.update_ast("file:///Collection.php", collection_stub);

    // The actual sandbox file content (slightly simplified but preserving
    // the structural patterns that trigger the crash).
    let content = r#"<?php

namespace App\Http\Controllers\Economy;

use Illuminate\Database\Query\Builder;
use Illuminate\Support\Facades\DB;
use stdClass;

class ExtractionToolController
{
    /**
     * @return array<string, mixed>
     */
    public function extraction_json(string $from_date, string $to_date): array
    {
        $subscriptionGateways = DB::table('subscriptions')
            ->select(DB::raw('gateway'))
            ->leftJoin('users', 'users.id', '=', 'subscriptions.user_id')
            ->where('users.country', 'dk')
            ->where('subscriptions.user_id', '>', 0)
            ->where('subscriptions.created', '>=', $from_date)
            ->where('subscriptions.created', '<=', $to_date)
            ->groupBy('gateway')->pluck('gateway');

        $numbers = [
            'sub_price'             => 0,
            'sub_price_without_vat' => 0,
        ];

        foreach ($subscriptionGateways as $gateway) {
            $tmpNumbers = DB::table('subscriptions')
                ->select(DB::raw('
                    SUM(subscriptions.price) AS sub_price,
                    (SUM(subscriptions.price) * (100/(100+vat_percentage))) AS sub_price_without_vat'))
                ->join('users', 'users.id', '=', 'subscriptions.user_id')
                ->where('users.country', 'dk')
                ->where('gateway', $gateway)
                ->where('subscriptions.user_id', '>', 0)
                ->where('subscriptions.created', '>=', $from_date)
                ->where('subscriptions.created', '<=', $to_date)
                ->where(function (Builder $query): void {
                    $query->whereNull('is_paid')
                        ->orWhere('is_paid', 1);
                })
                ->first();

            $numbers['sub_price'] = $tmpNumbers->sub_price;
        }

        $orders = DB::table('orders')
            ->select(DB::raw('
                SUM(amount) AS amount,
                SUM(postage) AS postage'))
            ->where('country', 'dk')
            ->where('created', '>=', $from_date)
            ->where('created', '<=', $to_date)
            ->first();

        $cancelled_orders = DB::table('orders')
            ->select(DB::raw('
                SUM(amount) AS amount'))
            ->where('country', 'dk')
            ->where('created', '>=', $from_date)
            ->where('created', '<=', $to_date)
            ->first();

        $data = [];
        $data['subscription_sales'] = $numbers['sub_price'];
        $data['postage_sales'] = $orders->postage;
        $data['cancelled_orders'] = $cancelled_orders;
        $data['from_date'] = $from_date;
        $data['to_date'] = $to_date;

        return $data;
    }
}
"#;

    backend.update_ast(uri, content);

    // ── Hover at various points along the deep chains ──
    // These should all complete without stack overflow.

    // Line 16: `$subscriptionGateways = DB::table(...)`
    // Hover on `table`
    hover_at(&backend, uri, content, 16, 50);

    // Line 17: `->select(DB::raw('gateway'))`
    // Hover on `select`
    hover_at(&backend, uri, content, 17, 15);

    // Line 23: `->groupBy('gateway')->pluck('gateway');`
    // Hover on `pluck`
    hover_at(&backend, uri, content, 23, 40);

    // Line 44: inside the closure: `$query->whereNull('is_paid')`
    // Hover on `whereNull`
    hover_at(&backend, uri, content, 44, 25);

    // Line 45: `->orWhere('is_paid', 1);`
    // Hover on `orWhere`
    hover_at(&backend, uri, content, 45, 25);

    // Line 47: `->first();`
    // Hover on `first` at end of the big chain
    hover_at(&backend, uri, content, 47, 18);

    // Line 57: `->first()` on orders chain
    hover_at(&backend, uri, content, 57, 15);

    // Line 65: `->first()` on cancelled_orders chain
    hover_at(&backend, uri, content, 65, 15);
}

/// Regression test: an extremely long chain (15+ calls) must not overflow.
#[test]
fn extremely_long_method_chain_does_not_crash() {
    let backend = create_test_backend();
    let uri = "file:///long_chain.php";

    let content = r#"<?php
class Builder {
    /** @return static */
    public function where(string $col, mixed $val = null): static { return $this; }
    /** @return static */
    public function andWhere(string $col, mixed $val = null): static { return $this; }
    /** @return static */
    public function orderBy(string $col): static { return $this; }
    /** @return static */
    public function limit(int $n): static { return $this; }
    /** @return static */
    public function offset(int $n): static { return $this; }
    /** @return array */
    public function get(): array { return []; }

    public static function query(): static { return new static(); }
}

class Repo {
    public function run(): void {
        $result = Builder::query()
            ->where('a', 1)
            ->where('b', 2)
            ->where('c', 3)
            ->where('d', 4)
            ->where('e', 5)
            ->where('f', 6)
            ->where('g', 7)
            ->where('h', 8)
            ->andWhere('i', 9)
            ->andWhere('j', 10)
            ->andWhere('k', 11)
            ->andWhere('l', 12)
            ->orderBy('a')
            ->limit(10)
            ->offset(20)
            ->get();
    }
}
"#;

    backend.update_ast(uri, content);

    // Hover on `get()` at the end of the 15+ call chain.
    hover_at(&backend, uri, content, 38, 15);

    // Hover on `where('h', 8)` in the middle of the chain.
    hover_at(&backend, uri, content, 30, 15);

    // Hover on `Builder::query()` at the start.
    hover_at(&backend, uri, content, 23, 30);
}

/// Regression test: multiple deep chains in the same method, each
/// assigning to a different variable, must not cause exponential
/// blowup or crash.
#[test]
fn multiple_deep_chains_same_method_does_not_crash() {
    let backend = create_test_backend();
    let uri = "file:///multi_chain.php";

    let content = r#"<?php
class QB {
    /** @return static */
    public function select(string ...$cols): static { return $this; }
    /** @return static */
    public function where(string $col, mixed $val = null): static { return $this; }
    /** @return static */
    public function join(string $table, string $a, string $op, string $b): static { return $this; }
    /** @return static */
    public function groupBy(string ...$cols): static { return $this; }
    /** @return ?object */
    public function first(): ?object { return null; }
    /** @return array */
    public function get(): array { return []; }
    public static function table(string $t): static { return new static(); }
}

class Report {
    public function generate(): void {
        $a = QB::table('t1')
            ->select('x', 'y')
            ->where('status', 1)
            ->where('type', 'foo')
            ->join('t2', 't1.id', '=', 't2.fk')
            ->groupBy('x')
            ->first();

        $b = QB::table('t2')
            ->select('a', 'b', 'c')
            ->where('active', true)
            ->where('deleted', false)
            ->where('archived', false)
            ->get();

        $c = QB::table('t3')
            ->where('x', 1)
            ->where('y', 2)
            ->where('z', 3)
            ->first();

        $d = QB::table('t4')
            ->select('*')
            ->join('t5', 't4.id', '=', 't5.ref')
            ->join('t6', 't5.id', '=', 't6.ref')
            ->where('t4.status', 'active')
            ->where('t5.flag', true)
            ->where('t6.valid', true)
            ->groupBy('t4.id')
            ->get();

        // Access all four results — each must resolve without crash.
        $x = $a;
        $y = $b;
        $z = $c;
        $w = $d;
    }
}
"#;

    backend.update_ast(uri, content);

    // Hover on each variable near the end of the method.
    let lines: Vec<&str> = content.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if line.contains("$x = $a") {
            hover_at(&backend, uri, content, i as u32, 10);
        }
        if line.contains("$y = $b") {
            hover_at(&backend, uri, content, i as u32, 10);
        }
        if line.contains("$z = $c") {
            hover_at(&backend, uri, content, i as u32, 10);
        }
        if line.contains("$w = $d") {
            hover_at(&backend, uri, content, i as u32, 10);
        }
    }
}

/// Focused regression test for the exact pattern that caused the
/// sandbox.php crash: `$var['key'] = $var['key']->method(...)`.
///
/// The raw-type inference path (`check_expression_for_raw_type`) did
/// not reduce `cursor_offset` before resolving the RHS, so resolving
/// `$var['key']` on the RHS re-entered `resolve_variable_assignment_raw_type`
/// for `$var` with the same cursor_offset, re-discovered the same
/// assignment, and recursed infinitely until stack overflow.
#[test]
fn self_referential_array_key_assignment_does_not_crash() {
    let backend = create_test_backend();
    let uri = "file:///self_ref_array.php";

    let content = r#"<?php
class Decimal {
    public function add(Decimal $other): Decimal { return $this; }
    public function sub(Decimal $other): Decimal { return $this; }
}

class Converter {
    public static function toDecimal(mixed $v): Decimal { return new Decimal(); }
}

class Demo {
    public function run(): void {
        $numbers = [
            'price'       => new Decimal(),
            'price_no_vat' => new Decimal(),
        ];

        $numbers['price'] = $numbers['price']->add(Converter::toDecimal(100));
        $numbers['price_no_vat'] = $numbers['price_no_vat']->sub(Converter::toDecimal(20));

        $x = $numbers;
    }
}
"#;

    backend.update_ast(uri, content);

    // Hover on `$numbers` at the self-referential assignment line.
    // Before the fix this caused infinite recursion → stack overflow.
    let lines: Vec<&str> = content.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if line.contains("$numbers['price'] = $numbers['price']->add") {
            let col = line.find("$numbers").unwrap_or(0) as u32;
            hover_at(&backend, uri, content, i as u32, col);
            break;
        }
    }

    // Also hover on `$x = $numbers` to resolve the full variable type.
    for (i, line) in lines.iter().enumerate() {
        if line.contains("$x = $numbers") {
            let col = line.find("$numbers").unwrap_or(10) as u32;
            hover_at(&backend, uri, content, i as u32, col);
            break;
        }
    }
}

/// Regression test: closure inside a chained method call where the
/// closure's parameter has no type hint (requires callable param
/// inference from the receiver chain).
#[test]
fn foreach_value_variable_shadows_iterator_receiver_does_not_crash() {
    let backend = create_test_backend();
    let uri = "file:///foreach_shadow.php";

    // `foreach ($category->getBranch() as $category)` — the foreach value
    // variable `$category` shadows the iterator receiver `$category`.
    // Resolving the value type tries to resolve the iterator expression
    // `$category->getBranch()`, which resolves `$category`, which finds
    // the same foreach again → infinite recursion without the depth guard.
    let content = r#"<?php
class Category {
    /** @return Category[] */
    public function getBranch(): array { return []; }
    public function getId(): int { return 1; }
}

class SiteTreeService {
    /** @param int[] $openCategories */
    private function getOpenCategories(array $openCategories, ?int $selectedId = null): array {
        if (null !== $selectedId) {
            $category = new Category();
            if ($category) {
                foreach ($category->getBranch() as $category) {
                    $openCategories[] = $category->getId();
                }
            }
        }
        return $openCategories;
    }
}
"#;

    backend.update_ast(uri, content);

    // Trigger unknown-member diagnostics — the overflow originally
    // happened in the diagnostics pipeline when resolving `$category`.
    let mut diags = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, content, &mut diags);

    // The test passes if we reach this point without a stack overflow.
    // `getId()` exists on Category, so no unknown-member diagnostic
    // should be emitted for it.
    assert!(
        !diags.iter().any(|d| d.message.contains("getId")),
        "getId should be known on Category, got: {:?}",
        diags
    );

    // Also trigger hover on `$category->getId()` inside the foreach
    // body to exercise the completion/hover path.
    let lines: Vec<&str> = content.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if line.contains("$category->getId()") {
            let col = line.find("$category").unwrap_or(0) as u32;
            hover_at(&backend, uri, content, i as u32, col);
            break;
        }
    }
}

#[test]
fn closure_without_type_hint_in_deep_chain_does_not_crash() {
    let backend = create_test_backend();
    let uri = "file:///closure_chain.php";

    let content = r#"<?php
class Builder {
    /** @return static */
    public function where(mixed $col, mixed $val = null): static { return $this; }
    /** @return static */
    public function whereNull(string $col): static { return $this; }
    /** @return static */
    public function orWhere(mixed ...$args): static { return $this; }
    /** @return static */
    public function select(mixed ...$cols): static { return $this; }
    /** @return static */
    public function join(string $t, string $a, string $op, string $b): static { return $this; }
    /** @return ?object */
    public function first(): ?object { return null; }
    public static function table(string $t): static { return new static(); }
}

class Controller {
    public function action(): void {
        $result = Builder::table('orders')
            ->select('*')
            ->join('users', 'orders.user_id', '=', 'users.id')
            ->where('status', 'active')
            ->where(function ($query): void {
                $query->whereNull('deleted_at')
                    ->orWhere('deleted_at', '0000-00-00');
            })
            ->where(function ($inner): void {
                $inner->where('type', 'premium')
                    ->orWhere(function ($deep): void {
                        $deep->where('type', 'trial')
                            ->where('expired', false);
                    });
            })
            ->first();

        $x = $result;
    }
}
"#;

    backend.update_ast(uri, content);

    // Hover on `first()` at the end of the chain with nested closures.
    let lines: Vec<&str> = content.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if line.contains("->first()") {
            hover_at(&backend, uri, content, i as u32, 15);
            break;
        }
    }

    // Hover inside the deepest nested closure.
    for (i, line) in lines.iter().enumerate() {
        if line.contains("$deep->where('type', 'trial')") {
            hover_at(&backend, uri, content, i as u32, 25);
            break;
        }
    }
}
