use crate::common::message_text;
use crate::common::{create_test_backend, hover_at, with_parse_worker_stack};
use tower_lsp_server::gen_lsp_types::Position;

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

/// Regression test: a generated-length fluent chain must resolve without
/// overflowing the stack.
///
/// A receiver spine is a chain, not a tree: every link's receiver is the
/// link before it, so walking one from the outermost call inward costs a
/// stack frame per link.  At this length that exhausts the stack every
/// AST-walking thread in the server gets, and a stack overflow is a
/// `SIGSEGV` that aborts the process rather than a panic a test harness can
/// catch.  Hand-written PHP stays short, but generated query builders and
/// generated API clients do not.
///
/// It runs on a parse-worker-sized stack because the libtest default is a
/// quarter of what the server gives these threads.
#[test]
fn generated_length_method_chain_does_not_overflow() {
    const LINKS: usize = 3000;

    let mut content = String::from(
        "<?php
class Fluent {
    /** @return static */
    public function self(): static { return $this; }
    public function finish(): int { return 0; }
}

class Runner {
    public function run(Fluent $start): void {
        $out = $start",
    );
    for _ in 0..LINKS {
        content.push_str("->self()");
    }
    content.push_str(";\n        $out->finish();\n    }\n}\n");

    // The chain sits on one line; `->self()` is eight characters wide, so
    // the last link's method name starts after the assignment prefix, every
    // earlier link, and that link's own arrow.
    let last_link_character = ("        $out = $start".len() + (LINKS - 1) * 8 + 2) as u32;

    let (chain_hover, assigned_hover) = with_parse_worker_stack(move || {
        let backend = create_test_backend();
        let uri = "file:///generated_chain.php";
        backend.update_ast(uri, &content);
        (
            // The last `self()` of the chain — resolving it walks every
            // link back to `$start`.
            backend.handle_hover(
                uri,
                &content,
                Position {
                    line: 9,
                    character: last_link_character,
                },
            ),
            // `finish()` on the assigned variable, which is only found if
            // the whole chain resolved to `Fluent`.
            backend.handle_hover(
                uri,
                &content,
                Position {
                    line: 10,
                    character: 16,
                },
            ),
        )
    });

    let chain_hover = format!(
        "{:?}",
        chain_hover.expect("hover on the last link of the chain")
    );
    assert!(
        chain_hover.contains("Fluent"),
        "The receiver of a {LINKS}-link chain should resolve to Fluent, got: {chain_hover}"
    );
    let assigned_hover = format!(
        "{:?}",
        assigned_hover.expect("hover on a method called on the chain's result")
    );
    assert!(
        assigned_hover.contains("finish"),
        "A {LINKS}-link chain should resolve to Fluent so finish() is found, got: {assigned_hover}"
    );
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
        !diags
            .iter()
            .any(|d| message_text(&d.message).contains("getId")),
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

/// A `\xNN` escape decodes to a byte no UTF-8 string can hold, so the
/// literal's *value* is arbitrary bytes rather than source text.  Reading
/// one through the unchecked `bytes_to_str` was undefined behaviour, and
/// aborted the process with a non-unwinding panic in debug builds.
///
/// The literals here reach the narrowing subject keys, array-shape keys,
/// regex shapes, `compact()` name collection, `define()` extraction, and
/// the docblock/interpolation readers.
#[test]
fn non_utf8_string_escapes_do_not_crash() {
    let backend = create_test_backend();
    let uri = "file:///non_utf8.php";

    let content = r#"<?php

define("BAD\x8bCONST", 1);

function probe(string $output, array $rows): void
{
    if (($pos = strpos($output, "\x8b")) !== false) {
        echo $pos;
    }

    $map = ["\x8b" => 'a', 'ok' => 'b'];
    echo $map["\x8b"];
    echo $map['ok'];

    $joined = "prefix" . "\x8b" . "suffix";
    echo $joined;

    if (preg_match("/\x8b(?<name>\d+)/", $output, $m)) {
        echo $m['name'];
    }

    foreach ($rows as ["\x8b" => $first, 'id' => $id]) {
        echo $first, $id;
    }

    $names = compact("\x8b");
    echo count($names);

    echo constant("BAD\x8bCONST");
    echo strlen("\212\377");
}

class Holder
{
    /** @var array<string, string> */
    private array $bag = ["\x8b" => 'v'];

    public function get(): string
    {
        return $this->bag["\x8b"];
    }
}

$h = new Holder();
echo $h->get();
probe("\x8b", []);
"#;

    // Parsing and symbol-map extraction must not abort.
    backend.update_ast(uri, content);

    // Diagnostics walk every expression, which is the path that aborted.
    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(uri, content, &mut diagnostics);

    // Hover exercises the same conversions outside a diagnostic pass.
    for (i, line) in content.lines().enumerate() {
        let col = line.len().min(20) as u32;
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

/// Regression test: generated-length operator chains must be indexed,
/// diagnosed, and answered on without overflowing the stack.
///
/// Binary operators nest on their left (`"a" . "b" . "c"` is
/// `("a" . "b") . "c"`), so a walker that recurses into each operator's left
/// operand costs a stack frame per operator, and a generated file can chain
/// tens of thousands of them.  Like a fluent chain (see
/// `generated_length_method_chain_does_not_overflow`), the overflow aborts
/// the process.
#[test]
fn generated_length_operator_chains_do_not_overflow() {
    const OPERANDS: usize = 20_000;

    let chain = |operand: &str, operator: &str| vec![operand; OPERANDS].join(operator);
    let content = format!(
        "<?php
function chains($v): string {{
    $sum = {};
    $sum->total();
    if ({}) {{ }}
    if ({}) {{ }}
    return {};
}}
return {};
",
        chain("1", " + "),
        chain("$v", " && "),
        chain("$v", " || "),
        chain("$v", " . "),
        chain("'a'", " . "),
    );
    // The middle of the chain on the function's `return` line.
    let return_line = 6;
    let mid_chain = (4 + "return ".len() + OPERANDS / 2 * 5) as u32;

    let (diagnostics, selection, folding) = with_parse_worker_stack(move || {
        // A plain PHP project, the reporter's setup: no `composer.json`, so
        // no framework support is switched on.
        let backend = create_test_backend();
        let root = tempfile::tempdir().expect("temp dir");
        crate::common::block_on(crate::common::initialize_with_options(
            &backend,
            root.path(),
            None,
        ));
        let uri = "file:///generated_operator_chains.php";
        backend.update_ast(uri, &content);
        let mut diagnostics = Vec::new();
        backend.collect_fast_diagnostics(uri, &content, &mut diagnostics);
        backend.collect_slow_diagnostics(uri, &content, &mut diagnostics);
        // Code actions offer nothing on an all-variable chain; getting an
        // answer back without aborting is the check.
        crate::common::get_code_actions_at(&backend, uri, &content, return_line, mid_chain);
        let position = Position::new(return_line, mid_chain);
        (
            diagnostics,
            backend.handle_selection_range(&content, &[position]),
            backend.handle_folding_range(uri, &content),
        )
    });

    // `$sum` is only known to be an int if the whole chain was folded.
    let messages: Vec<&str> = diagnostics
        .iter()
        .map(|d| message_text(&d.message))
        .collect();
    assert!(
        messages
            .iter()
            .any(|message| message.contains("total()") && message.contains(&OPERANDS.to_string())),
        "The {OPERANDS}-operand sum should resolve to {OPERANDS}, got: {messages:#?}"
    );

    // The chain is one expression: expanding from an operand selects the
    // whole chain, not each of its thousands of prefixes.
    let selection = selection.expect("selection ranges").remove(0);
    let mut levels = 0;
    let mut range = Some(&selection);
    while let Some(current) = range {
        levels += 1;
        range = current.parent.as_deref();
    }
    assert!(
        levels < 10,
        "A flat chain should take a handful of selection levels, got {levels}"
    );

    assert!(folding.is_some(), "Folding ranges should be computed");
}

/// The framework scanners walk every file a Laravel project hands them, so
/// a generated chain in a provider, a route file, a config file, or a form
/// request must not overflow their walks either.
#[test]
fn generated_length_operator_chains_do_not_overflow_framework_scanners() {
    use tower_lsp_server::LanguageServer;
    const OPERANDS: usize = 20_000;

    let chain = vec!["'a'"; OPERANDS].join(" . ");
    let provider = format!(
        "<?php
namespace App\\Providers;

use Illuminate\\Support\\ServiceProvider;

class AppServiceProvider extends ServiceProvider
{{
    public function boot(): void
    {{
        $generated = {chain};
    }}
}}
"
    );
    let request = format!(
        "<?php
namespace App\\Http\\Requests;

use Illuminate\\Foundation\\Http\\FormRequest;

class ChainRequest extends FormRequest
{{
    public function rules(): array
    {{
        return ['name' => {chain}];
    }}

    public function store(): void
    {{
        $this->validate(['name' => {chain}]);
        $this->
    }}
}}
"
    );
    let files = [
        ("app/Providers/AppServiceProvider.php", provider),
        ("app/Http/Requests/ChainRequest.php", request.clone()),
        ("routes/web.php", format!("<?php\n$generated = {chain};\n")),
        (
            "config/app.php",
            format!("<?php\nreturn ['aliases' => ['Generated' => {chain}]];\n"),
        ),
    ];
    // The line holding `$this->` and the column just past it.
    let completion_line = 15;
    let completion_column = 15;

    with_parse_worker_stack(move || {
        let file_refs: Vec<(&str, &str)> = files
            .iter()
            .map(|(path, content)| (*path, content.as_str()))
            .collect();
        let (backend, dir) = crate::common::create_psr4_workspace(
            r#"{"require":{"laravel/framework":"^13.0"},"autoload":{"psr-4":{"App\\":"app/"}}}"#,
            &file_refs,
        );
        crate::common::block_on(async {
            backend
                .initialized(tower_lsp_server::gen_lsp_types::InitializedParams {})
                .await;
            let uri = crate::common::open_php_at(
                &backend,
                &dir,
                "app/Http/Requests/ChainRequest.php",
                &request,
            )
            .await;
            let mut diagnostics = Vec::new();
            backend.collect_fast_diagnostics(uri.as_str(), &request, &mut diagnostics);
            backend.collect_slow_diagnostics(uri.as_str(), &request, &mut diagnostics);
            crate::common::complete_at_raw(
                &backend,
                &uri,
                &request,
                completion_line,
                completion_column,
            )
            .await;
            crate::common::get_code_actions_at(
                &backend,
                uri.as_str(),
                &request,
                9,
                (25 + OPERANDS / 2 * 6) as u32,
            );
        });
    });
}
