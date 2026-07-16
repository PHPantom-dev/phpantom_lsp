# PHPantom — Bug Fixes

Every bug below must be fixed at its root cause. "Detect the
symptom and suppress the diagnostic" is not an acceptable fix.
If the type resolution pipeline produces wrong data, fix the
pipeline so it produces correct data. Downstream consumers
(diagnostics, hover, completion, definition) should never need
to second-guess upstream output.

All entries below come from the 2026-07 analyze triage sweep over
the sample projects (see `projects/analyze-triage.md`). Except
where noted, each was reproduced in isolation with a minimal
fixture against a debug build. Counts are the number of analyze
errors the bug accounts for across the sample projects and are
approximate — fixing an upstream bug often clears cascading
errors attributed to other buckets.

Laravel-specific items from the same sweep are in
`docs/todo/laravel.md` (L21 alias parsing); ~50 further errors
were reclassified as intended
diagnostics per the declared-types philosophy there. The closure
literal-return shape gap is filed as T31 in
`docs/todo/type-inference.md`.

The B91–B108 batch below comes from the 2026-07-16 full re-triage of
the three remaining non-clean projects (PDepend, Luxplus Website,
Luxplus Backoffice). Together they account for every remaining
analyze error in those projects (42 errors after the same sweep's
project-side patches). Each was reproduced with the minimal fixture
shown unless noted otherwise. Note that `unresolved_member_access`
is off by default — fixtures must be run in a project whose
`.phpantom.toml` sets `[diagnostics] unresolved-member-access = true`,
matching the sample projects.

## B91. Narrowing guards do not apply to array-index subject expressions

**Severity: Medium (~5 errors: pdepend ×3, luxplus-backoffice ×2) · Reproduced with fixture**

```php
/** @psalm-assert =ExpectedType $actual */ // PHPUnit's assertInstanceOf
static::assertInstanceOf(Wanted::class, $constants['C']);
$constants['C']->getImage(); // "type of '$constants['C']' could not be resolved"

if (!is_a($config['class'], Extension::class, true)) {
    throw new RuntimeException('nope');
}
$m->activateExtension($config['class']); // "expects class-string<Extension>, got string"
```

Assert-tag narrowing (`assertInstanceOf`) and `is_a(..., true)`
class-string narrowing both work when the subject is a plain
variable, but are silently dropped when the subject is an array
index expression (`$arr['key']`, `$arr[0]`). PHPStan keys narrowing
by printed expression, so index expressions narrow like any other
subject. Covers PDepend's `PdependExtension.php:87`
`type_mismatch_argument` and the `$constants['C']`/`$elements[0]`
accesses in its PHP 8.2 enum tests, plus the two
`$parameters['amount']->toFixed(2)` accesses in Backoffice's
BusinessCentral tests.

## B92. Assert narrowing cannot override variables assigned by list-destructuring from an unresolvable RHS

**Severity: Medium (~8 errors, pdepend) · Reproduced with fixture**

```php
[$type, $variable] = $declarations[0]; // RHS type unknown (bare array param)
static::assertInstanceOf(Wanted::class, $type);
$type->getImage(); // "type of '$type' could not be resolved"
```

A plain assignment from the same unresolvable RHS
(`$type = $declarations[0];`) narrows fine, and destructuring from a
*typed* RHS works. Only the combination — list-destructure whose RHS
cannot be resolved — leaves the variables in a state that later
assert narrowing cannot override. Accounts for all 8 `getImage()`
errors in PDepend's parser tests.

## B93. A `for` loop's init-clause assignment is invisible to the condition and update clauses

**Severity: Low (1 error, pdepend) · Reproduced with fixture**

```php
for ($previous = $e->getPrevious(); $previous; $previous = $previous->getPrevious()) {
    echo $previous->getMessage(); // body resolves fine
}
// update clause: "type of '$previous' could not be resolved"
```

The loop body sees `$previous`, but the update expression on the
`for` line itself does not, so the diagnostic fires on the `for`
statement. Rewriting as a `while` loop resolves. PDepend
`src/TextUI/Command.php:288`.

## B94. A closure parameter's declared union type is overridden by the inferred collection element type

**Severity: Medium (shares 3 errors with B95, luxplus-website) · Reproduced with fixture**

```php
/** @param Collection<int, CanApply>|Collection<int, ViewModel>|Collection<int, stdClass> $items */
public function probe(Collection $items): void
{
    $items->filter(function (CanApply|ViewModel|stdClass $item): bool {
        if (isset($item->salesCampaignGroupId)) {
            return $item->salesCampaignGroupId === 1; // "Property ... not found on class 'CanApply'"
        }
        return false;
    });
}
```

When the subject is a union of differently-parameterized
collections, the closure parameter collapses to the first union
member's element type (`CanApply`), discarding the parameter's own
declared union. A declared parameter type must win over (or at
least union with) the inferred element type. Website
`SalesCampaignGroupDiscountService.php`.

## B95. `isset($obj->prop)` guards and `property_exists()` ternaries do not prove the property on a single-typed subject

**Severity: Medium (shares 3 errors with B94, luxplus-website) · Reproduced with fixture**

```php
function probeIsset(CanApply $item): int
{
    if (isset($item->salesCampaignGroupId)) {
        return $item->salesCampaignGroupId; // "Property ... not found on class 'CanApply'"
    }
    return 0;
}

function probeTernary(CanApply $item): mixed
{
    return property_exists($item, 'qty') ? $item->qty : 1; // same
}
```

`property_exists()` in an `if` statement already proves the member
(shipped earlier); the ternary form does not, and `isset($obj->prop)`
does not in either form. PHPStan treats both as existence proofs for
the guarded access.

## B96. `reduce()`'s return template is not bound from the closure's inferred return type

**Severity: Medium (1 error, luxplus-website) · Reproduced with fixture**

```php
/**
 * @template TReduceInitial
 * @template TReduceReturnType
 * @param callable(TReduceInitial|TReduceReturnType, TValue, TKey): TReduceReturnType $callback
 * @param TReduceInitial $initial
 * @return TReduceReturnType
 */
public function reduce(callable $callback, $initial = null) {}

$total = $items->reduce(fn(Decimal $carry, $op) => $carry->add($op->getPrice()), new Decimal('0'));
$total->toFixed(2); // "type of '$total' could not be resolved"
```

`TReduceReturnType` only appears in the callable's return position,
so binding it requires inferring the closure's return type
(`Decimal`, from `$carry->add(...)`). Website
`OrderDetailedResource.php:155`.

## B97. Array element access with a dynamic (non-literal) key does not resolve the element type

**Severity: Medium (~4 errors: luxplus-website ×1, luxplus-backoffice ×2, plus 1 cascade) · Reproduced with fixture**

```php
/** @return array{normalPrice: Decimal, memberPrice: Decimal} */
private function getPrices(): array {}

$prices = $this->getPrices();
$price = $prices[$priceToUse] ?? null;      // $priceToUse: string
if ($price === null) { return ''; }
$price->toString(); // "type of '$price' could not be resolved" — literal key works

// Same family with a loop-built map of shapes:
$sums[$id] = $this->getStructure();          // array{bonusCashPaid: Decimal, ...}
$sums[$id]['bonusCashPaid']->add($paid);     // unresolved

// And nested dynamic writes read back across iterations:
$return['data'][$count]['earnings'] = $price;
$sum = $return['data'][$count]['earnings'];  // unresolved
$sum->add($price);
```

Indexing a shape (or a homogeneous map built in a loop) with a
variable key should resolve to the union of the value types.
Website `ProductRoutineTest.php:163`, Backoffice
`RAFEventsAggregatorService.php:108` and `EconomyController.php:540`.

## B98. Full-project analyze nondeterministically fails to resolve closure parameters that resolve in single-file runs

**Severity: Medium (0-10 errors per run: luxplus-website ×3, luxplus-backoffice ×1) · Observed repeatedly, no isolated fixture possible**

```php
// UsersController.php — errors appear only in some full-project runs:
$userInfo['skin_concerns'] = array_map(fn($e) => $e->value, $request->skinConcerns);
// "type of '$e' could not be resolved" — file analyzed alone is always clean
```

During the 2026-07-16 re-triage, Website's three `fn($e) => $e->value`
errors (UsersController) appeared in the first full-project run and
were absent from six later identical runs; Backoffice's
`fn($cause) => $cause->value` error (CreateRefund.php:141) appears in
full runs but never single-file. Same binary, same project state,
different diagnostics — which files get analyzed together (worker
scheduling) changes resolution results. Points at shared state
(thread-locals or consumer-gated caches — see performance
anti-patterns #4/#5 in `AGENTS.md`) leaking between files in the
parallel analyze pipeline. Also makes the analyze-triage error
counts themselves unstable by a few errors between runs.

## B99. An `array<T>|false` union loses the array's element type

**Severity: Medium (2 errors, luxplus-backoffice) · Reproduced with fixture**

```php
/** @return array<int, self>|false */
public static function getColumns(bool $x): array|false {}

$columns = Col::getColumns($x);
if (!is_array($columns)) { return; }   // === false check fails the same way
foreach ($columns as $column) {
    echo $column->value; // "type of '$column' could not be resolved"
}
```

The nullable equivalent (`array<int, self>|null` with a `!$columns`
guard) resolves fine; only the `|false` union drops the element
type. Also swallows the docblock when it refines a native
`array|false` return. Backoffice `ProductPriceSheetService.php`.

## B100. Leading-backslash global function calls do not resolve in member chains

**Severity: Low (1 error, luxplus-backoffice) · Reproduced with fixture**

```php
return \response()->json([...]); // "type of '\response()' could not be resolved"
return response()->json([...]);  // works
```

## B101. Reassignment inside a guard branch discards the other path's type when the initial type is partially unresolved

**Severity: Medium (2 errors, luxplus-backoffice) · Reproduced with fixture (enum case); session() case reproduced in-project**

```php
$type = $b ? Country::ADMIN : $m->grab(); // grab(): mixed
if (!$type || !$type instanceof Country) {
    $type = Country::ADMIN;
}
$type->getFlagImageName(); // "type of '$type' could not be resolved"

// Same family (reproduces with session('file') in a Laravel project):
$file = session('file');
if (!is_string($file)) { $file = null; }
if ($file) {
    explode('/', $file); // "Argument 2 ($string) expects string, got null"
}
```

After the guard, the variable should be the union of the branch
assignment and the narrowed fall-through type. When the initial
type has an unresolved component, the fall-through contribution is
dropped and only the branch assignment (`null` / the enum) survives
— in the first case producing no type at all, in the second
narrowing to literal `null` inside a branch that proves the
variable truthy. Both simple-fixture equivalents with fully-known
initial types work. Backoffice `AuditViewModel.php:54`,
`InvoiceAnalyzerController.php:57`.

## B102. Conditional return types with `static<...>` generic branches do not resolve

**Severity: Medium (1 error, luxplus-backoffice) · Reproduced with fixture**

```php
/** @return ($preserveKeys is true ? static<int, static> : static<int, static<int, TValue>>) */
public function chunk($size, $preserveKeys = true) {}

foreach ($tokens->chunk(500) as $batch) {
    $batch->values(); // "type of '$batch' could not be resolved"
}
```

The same nested-`static` return type without the conditional wrapper
(`@return static<int, static>`) resolves fine; wrapping it in a
conditional breaks it whether the condition parameter is omitted
(default) or passed explicitly. This is Laravel's actual
`Collection::chunk()` docblock, so every `chunk()` call in a Laravel
project hits it. Backoffice `PushNotificationService.php:60`.

## B103. `return $this` in a trait method resolves to the trait instead of the using class

**Severity: Medium (2 errors, luxplus-backoffice) · Reproduced with fixture**

```php
trait MakesAssertions
{
    function assertSee(string $v) { return $this; } // no return type
}

class Testable { use MakesAssertions; public int $x = 1; }

$t->assertSee('a')->x; // "Property 'x' not found on class 'MakesAssertions'"
```

Body-inferred `return $this` works for plain class methods, but for
a trait method the chain continues with the *trait* as the subject,
so members of the using class (including its other traits and magic
`__call`) disappear after the first chained call. This is Livewire's
`Testable` shape, breaking every multi-step Livewire test assertion
chain. Backoffice `ShowReorderListTest.php:78`,
`ProductListTest.php:30`.

## B104. `instanceof` on the left of `&&` does not narrow an untyped closure parameter on the right

**Severity: Low-Medium (2 errors, luxplus-backoffice) · Reproduced with fixture**

```php
$response->assertViewHas('faqs', fn($faqs) => $faqs instanceof Collection && $faqs->contains($faq1));
// "type of '$faqs' could not be resolved" on the contains() call
```

Backoffice `FaqsControllerTest.php:156`.

## B105. An inline `@var` retype of a closure's `mixed` parameter is ignored before `foreach`

**Severity: Low (1 error, luxplus-backoffice) · Reproduced with fixture**

```php
function (mixed $subscriptions): bool {
    self::assertIsIterable($subscriptions);
    /** @var iterable<Subscription> $subscriptions */
    foreach ($subscriptions as $subscription) {
        $subscription->user_id; // "type of '$subscription' could not be resolved"
    }
}
```

Backoffice `SubscriptionsControllerTest.php:99`.

## B106. `assertTrue(property_exists(...))` does not prove the property exists

**Severity: Low (1 error, luxplus-backoffice) · Reproduced with fixture**

```php
$model = $response->viewData('model'); // mixed
self::assertIsObject($model);
self::assertTrue(property_exists($model, 'value'));
$model->value; // "type of '$model' could not be resolved"
```

The bare-`if` form of `property_exists()` narrowing shipped earlier;
the same proof wrapped in PHPUnit's `assertTrue()` (whose
`@psalm-assert true $condition` re-exports the inner condition) does
not fire. Backoffice `ApplicationSettingsControllerTest.php:99`.

## B108. A leading-backslash docblock type is resolved through a same-short-name import

**Severity: Low-Medium (2 errors, luxplus-backoffice) · Reproduced with fixture**

```php
use Illuminate\Support\Facades\Redis;

/** @var \Redis */
$client = $cache->client();
$client->select(1); // "Method 'select' not found on class 'Illuminate\Support\Facades\Redis'"
```

`\Redis` must resolve to the global class regardless of imports; the
leading backslash is being stripped before name resolution, so the
facade import wins. Backoffice `ApplicationSettingsController.php`
(annotated `@var \Redis` during the 2026-07-16 sweep precisely so
this resolves once fixed).
