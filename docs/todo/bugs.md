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

## B67. Positional array-shape indexing does not resolve the element type

**Severity: Medium-High (~20 errors, pdepend) · Confirmed with fixture**

```php
/** @var array{Label, Stmt} $pair */
$pair = $n->getChildren();
$pair[0]->getImage();   // "type of '$pair[]' could not be resolved"
```

Both single-line and multiline `@var array{...}` shapes fail
(pdepend `tests/.../PHP81/MatchExpressionTest.php` and several
other parser feature tests: `$pair[]`, `$children[]`,
`$elements[]`). This is the same symptom as the previously fixed
B58 — either the fix regressed or it never covered the
`@var`-annotation path; the old fix's tests should be extended.

## B68. Foreach over an Iterator subclass ignores the inherited generic value type

**Severity: Medium (~5 errors, pdepend) · Confirmed from output**

```php
/** @extends FilterIterator<int, SplFileInfo, \Iterator<int, SplFileInfo>> */
class Iterator extends FilterIterator { ... }

foreach ($fileIterator as $file) {
    $file->getRealPath();  // "Method 'getRealPath' not found on class 'PDepend\Input\Iterator'"
}
```

Iterating an object that implements `Iterator`/`IteratorAggregate`
should use the value type from the class's inherited generic
iterator parameters (or the `current()` return type as fallback).
Instead the element is typed as the iterator class itself, or not
at all. Also fails for direct SPL iteration
(`foreach (new DirectoryIterator(...) as $file)`, pdepend
`tests/php/PDepend/ParserRegressionTest.php:80`).

Note: the ~12 luxplus-backoffice paginator errors
(`foreach (ProductGroup::paginate(25) as $productGroup)`) initially
filed here were *not* this bug — they were a framework docblock gap
(`Builder::paginate()` declared an unparameterized
`LengthAwarePaginator`), now corrected so the paginators resolve
their element type through `IteratorAggregate`. This bug is only
the SPL / direct-iteration case above.

## B69. Indexing a call result inline breaks the rest of the chain

**Severity: Medium-High (~16 errors: pdepend ~9, luxplus-backoffice 7) · Confirmed with fixture**

```php
$a->findChildrenOfType(ASTAttribute::class)[0]->getParent();
// "type of '$a->findChildrenOfType(ASTAttribute::class)[]' could not be resolved"

Country::cases()[0]->value;   // same failure on enum cases()
```

Splitting into two statements (`$children = $a->findChildrenOfType(...);
$children[0]->getParent();`) works, so the array element type is
available — only the inline `call(...)[index]->member` chain form
fails in subject extraction/resolution.

## B70. Call-expression arguments are not resolved for template binding and callback parameter inference

**Severity: Medium-High (~12 errors: phpmd 2, pdepend ~5, luxplus-website 3, agcms 1, + masked variants) · Confirmed with fixture**

Two facets of the same call-site gap:

1. `@template T` binding from `array<T>` fails when the argument
   is a call expression (works for variables and array literals):

   ```php
   /** @template T  @param array<T> $a  @return T */
   function first(array $a): mixed { ... }

   $emails = first(self::getEmailConfigs());  // getEmailConfigs(): array<string, EmailConfig>
   $emails->address;   // "type of '$emails' could not be resolved"
   ```

2. Callback parameter types are not inferred when the array
   argument of `array_map`/`array_filter` is a call or property
   expression (works when it is a variable):

   ```php
   array_map(static fn($node) => $node->getImage(), $new->getChildren());
   //                            ^ "type of '$node' could not be resolved"
   ```

The root is the same: `build_function_template_subs`' generic
wrapper arm only resolves `$variable` arguments and array literals
(see T25 in `docs/todo/type-inference.md`, where the array-literal
case was added) — route argument resolution through the shared
`resolve_rhs_expression` pipeline instead of special-casing
argument syntax shapes.

Related scope defect confirmed by the same fixtures: when the
callback parameter shares its name with an outer variable, the
parameter silently borrows the *outer* variable's type instead of
failing (masking the gap and producing wrong types). Closure
parameters must shadow outer variables unconditionally.

A third facet of the same call-site gap: `app()->make($repository)`
where `$repository` is a foreach element of a literal
`[Foo::class, Bar::class, ...]` array — the declared
`class-string<T>` union never binds `make()`'s template, so the
chained call is unresolved (2 errors, luxplus-backoffice
`app/Jobs/SalesInfo/UpdateSalesInfoLocalJob.php:37`). All facts are
declared; only the argument-shape special-casing is in the way.

## B72. `compact()` with an array argument is not recognised, producing unused-variable false positives

**Severity: Medium (~6 errors, luxplus-website) · Confirmed with fixture**

```php
$activeEvents = getActiveEvents();
$showDefault = true;
$args = compact([
    'activeEvents',
    'showDefault',
]);
```

The unused-variable diagnostic treats `$activeEvents` and
`$showDefault` as unused because the `compact()` recogniser only
inspects direct string-literal arguments (`compact('a', 'b')`). When
`compact()` is passed a single array of names (`compact(['a', 'b'])`,
a form PHP supports and `compact()` documents), the array elements are
never collected, so every variable named inside the array is falsely
reported unused. The recogniser walks argument expressions but bottoms
out at the array literal instead of descending into its string
elements. Collect names from array-literal arguments (recursively, so
nested arrays also work) in the same place string arguments are
collected (luxplus-website
`app/Http/Controllers/FavoriteController.php:179-190`).

## B73. A variable used only as a dynamic method name is reported unused

**Severity: Low (~1 error, luxplus-backoffice) · Confirmed from output**

```php
$assertion = $cond ? 'assertSee' : 'assertDontSee';
$response->{$assertion}($value);
```

The unused-variable diagnostic flags `$assertion` as unused even
though it is read as the method name in `$response->{$assertion}(...)`.
The usage scan does not treat the braced method-name selector of a
method call as a read position, so a variable referenced only there is
missed. Walk the method-name expression of method / null-safe-method /
static-method calls when collecting variable reads (luxplus-backoffice
`tests/Feature/Products/Tariffs/TariffsTest.php:73`).

## B74. `App::make()` / `App::makeWith()` (facade form) does not resolve container bindings

**Severity: Medium (~8 errors, luxplus-website) · Confirmed with fixture**

```php
use Illuminate\Support\Facades\App;

app()->make(EventRepository::class)->getActiveEvents($country);       // resolves
App::make(EventRepository::class)->getActiveEvents($country);         // "could not be resolved"
App::makeWith(PageService::class, ['page' => $page]);                 // "could not be resolved"
```

The `app()->make(X::class)` helper-function form resolves the concrete
class and lets member access on the result type-check normally. The
equivalent `Illuminate\Support\Facades\App::make(X::class)` /
`App::makeWith(X::class, [...])` facade-static-call form does not —
member access chained directly off the facade call is unresolved, even
though `App` is just a thin static proxy to the same container `make()`
call. Route facade-form container resolution through the same shortcut
that already handles the helper-function form (luxplus-website
`app/Http/Controllers/PagesController.php`,
`app/Http/Controllers/LinkCampaignsController.php`,
`app/View/Components/ProductDiscovery/ProductDiscoveryModal.php`,
fixed in the test project by switching to `app()->make()` pending this
fix; see `analyze-triage.md`).

## B75. Eloquent relation property access is case-sensitive, unlike the underlying PHP method call

**Severity: Low (~2 errors, luxplus-website) · Confirmed with fixture**

```php
class Order extends Model
{
    public function orderProducts(): HasMany { /* ... */ }
}

$order->orderProducts;   // resolves
$order->orderproducts;   // "could not be resolved"
```

PHP method names are case-insensitive, so Eloquent's `__get()` magic
accessor correctly calls `orderProducts()` regardless of the case used
at the call site, and the relation resolves at runtime either way.
PHPantom's Laravel relation-property resolution only matches the exact
declared case, so a differently-cased (but functionally identical)
property access is reported as unresolved. Match relation properties
case-insensitively, the same way direct method calls already are
(luxplus-website
`app/Http/Resources/OrderDetailedResource.php:146-147`, via
`Order::orderProducts()` accessed as `$order->orderproducts`).

## B76. `$this` inside an anonymous class body resolves to the enclosing method's class instead of the anonymous class

**Severity: Low (~1 error, luxplus-website) · Confirmed with fixture**

```php
class Test
{
    public function make(): object
    {
        return new class (5) {
            public function __construct(private readonly int $value) {}

            public function get(): int
            {
                return $this->value;   // "Property 'value' not found on class 'Test'"
            }
        };
    }
}
```

Inside an anonymous class's own methods, `$this` must resolve to the
anonymous class, not to whatever class contains the `new class { ... }`
expression. The resolver appears to keep the enclosing method's class
as the current `$this` type when it descends into the anonymous
class's method bodies instead of switching context (luxplus-website
`tests/Feature/Web/LinkCampaignsControllerTest.php:347`, a mocked
service defined as an anonymous class inside a test helper method).
