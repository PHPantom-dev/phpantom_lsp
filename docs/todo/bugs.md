# PHPantom — Bug Fixes

Every bug below must be fixed at its root cause. "Detect the
symptom and suppress the diagnostic" is not an acceptable fix.
If the type resolution pipeline produces wrong data, fix the
pipeline so it produces correct data. Downstream consumers
(diagnostics, hover, completion, definition) should never need
to second-guess upstream output.

#### B33. Included route file paths behind a local variable are not resolved

**Impact: Low · Effort: Medium**

`resolve_path_arg` (`src/virtual_members/laravel/provider_resources.rs`)
now follows a `Variable` argument back to its most recent assignment in
the enclosing method when resolving `mergeConfigFrom`/`loadViewsFrom`/
`loadTranslationsFrom`/`loadRoutesFrom` paths, but `route_names.rs`'s
`open_included_file` (which resolves `require __DIR__ . '/x.php'` and
`Route::group([], base_path('x.php'))` targets while following include
chains during route scanning) calls `resolve_path_arg` with `program:
None`, so the same local-variable indirection is still unresolved there:

```php
$routes = __DIR__.'/../routes/api.php';
Route::group(['prefix' => 'v1'], $routes);
```

`open_included_file` never opens `routes/api.php`, so any route defined
in it is invisible to `route()` / route-name completion.

**Where to look:** `open_included_file` would need the already-parsed
`Program` threaded through `scan_route_file` → `scan_stmt` → `scan_expr`
→ `scan_included_file` (each currently receives only `content` and
`ScanPaths`), which is a larger, more invasive change than the
provider-resources fix since `Program` does not currently travel through
that call chain.

#### B38. `@props` and anonymous-component attributes are reported as undefined

**Impact: Medium · Effort: Low-Medium**

An anonymous Blade component receives every attribute the caller passes
as a local variable, and `@props` declares those variables explicitly.
Neither is recognised:

```blade
@props([
    'caption' => '',
])
<span>{{ $caption }}</span>   {{-- Undefined variable '$caption' --}}
```

`@props` is the easy half and should be read directly. Attributes with
no `@props` declaration need the caller's `<x-… :foo="$bar" />` tag, which
depends on component tag parsing.

#### B39. Multi-line PHP expressions in Blade component attributes are truncated

**Impact: Medium-High · Effort: Medium**

A `:attribute="…"` value that spans several source lines is cut short,
producing a syntax error at the wrap point plus a bogus
`argument_count_mismatch` for the call that got truncated:

```blade
<x-backoffice::file.upload name="image"
    :rules="[
        'Dimensions must match: 2420 x 1614',
        'Max file size: 2 mb',
    ]" />
```

```
Syntax error: unexpected token `)`
Syntax error: unexpected token `;`, expected `]`
```

14 diagnostics across two sample projects (10 `syntax_error` in one; 2
`syntax_error` plus 2 `argument_count_mismatch` in the other). Wrapping
long attribute expressions is what a formatter produces, so this fires
on ordinary, well-formatted templates.

#### B40. Assignments in the inline `@php(…)` directive are not recorded

**Impact: Medium · Effort: Low**

The block form updates scope correctly; the inline form does not:

```blade
@php($a = $order->orderProducts->whereIn('product_id', [1]))
{{ $a->isNotEmpty() }}   {{-- type of '$a' could not be resolved --}}

@php
    $b = $order->orderProducts->whereIn('product_id', [1]);
@endphp
{{ $b->isNotEmpty() }}   {{-- resolves --}}
```

#### B41. Translation-key diagnostics fire when the app replaces the translation loader

**Impact: Medium-High · Effort: Low-Medium**

The Trans arm skips the check when no translation files were found, but
an application that keeps its strings in the database still has
`vendor/`'s own `lang/` files on disk, so the set is non-empty and every
application key is reported as unknown.

```php
$this->app->singleton('translation.loader', fn ($app) => new DatabaseTranslationLoader(
    new FileLoader($app->make('files'), $app->make('path.lang')),
));
```

28 diagnostics in one sample project, none of them real.

**Fix:** when a service provider rebinds `translator` or
`translation.loader` to something other than Laravel's own `FileLoader`,
the valid-key set is unknowable — skip the check, the same way an
unenforced morph map does.

#### B43. `App::make()` / `App::makeWith()` with a class-string do not resolve

**Impact: Medium-High · Effort: Low**

The `app()` helper resolves a class-string argument to that class; the
`App` facade does not:

```php
app(CurrencyHelper::class)->noSuchMethod();          // resolves ✓
app()->make(CurrencyHelper::class)->noSuchMethod();  // resolves ✓
App::make(CurrencyHelper::class)->noSuchMethod();    // could not be resolved ✗
App::makeWith(CurrencyHelper::class, [])->…          // could not be resolved ✗
```

8 diagnostics in one sample project. Whatever gives the helper its
class-string return needs to apply to the facade's `make`, `makeWith`
and `resolve` too.

#### B44. String container bindings do not resolve

**Impact: Low-Medium · Effort: Medium**

`app()->make('sentry')` resolves to nothing, because nothing indexes the
string keys that service providers bind:

```php
// Sentry\Laravel\ServiceProvider
$this->app->singleton('sentry', fn () => new HubAdapter());
```

Provider scanning already walks `register()` for config and route
resources, so the `bind()` / `singleton()` / `instance()` calls with a
string abstract and a resolvable concrete are within reach.

#### B47. Deprecation diagnostics ignore the project's target PHP version

**Impact: Medium · Effort: Low-Medium**

A project pinned to PHP 8.4 (`"require": {"php": "^8.4"}`,
`"config": {"platform": {"php": "8.4"}}`) is reported for a deprecation
introduced in 8.5:

```
'Pdo\PDO::sqliteCreateFunction' is deprecated: use Pdo\Sqlite::createFunction
instead (since PHP 8.5)
```

6 diagnostics in one sample project. The stub already carries the
version the deprecation landed in; it should be compared against the
project's resolved target version, and the diagnostic suppressed when
the target predates it.

#### B48. `Collection::keyBy()` does not rebind the key template

**Impact: Medium · Effort: Medium**

`keyBy()` re-keys a collection, so its result is
`Collection<TNewKey, TValue>` where `TNewKey` comes from the callback's
return type (or the type of the named column). PHPantom keeps the
original `int` key from the Eloquent collection, so every subsequent
`get()` with the new key is reported:

```php
$byMarket = ProductPrice::query()->get()
    ->keyBy(fn (ProductPrice $pp): string => $pp->market->value);

$byMarket->get($translation->lang_code->value);
// Argument 1 ($key) expects int|null, got string
```

3 diagnostics across two sample projects (one of them through a Blade
view that receives the collection).

#### B49. Static call through a string-typed variable is reported as scalar access

**Impact: Medium · Effort: Low**

`$string::method()` is valid PHP — the string is the class name — but it
is reported as a member access on a scalar:

```php
$job->class_name::dispatch();
// Cannot access method 'dispatch' on type 'string'
```

A `class-string<T>` subject should resolve to `T`; a plain `string`
subject is unresolvable, which is a "cannot verify" at most, never a
scalar-access error.

#### B50. Closure parameter types are not narrowed from the call site

**Impact: Medium · Effort: Medium**

When a closure is passed to `array_map()` (and friends) over an array
with a known element type, that element type should refine a wider
declared parameter type. PHPStan does this at level max; PHPantom keeps
the declared `array`:

```php
/** @return iterable<array{DiscountType, ?CartLabel}> */
private static function yieldCases(): iterable { /* … */ }

array_map(
    static fn (array $case): string => $case[0]->name,   // type of '$case[0]' could not be resolved
    iterator_to_array(self::yieldCases()),
);
```

3 diagnostics in one sample project. Related to
[T25](type-inference.md#t25-call-site-template-argument-inference-for-callable-parameters),
which covers the template side of the same call-site inference.

#### B51. An `instanceof` result stored in a variable does not narrow

**Impact: Medium · Effort: Medium**

Narrowing works on a direct `instanceof` condition but is lost when the
result is assigned first:

```php
$isHtml = $raw instanceof HtmlString;

return $isHtml ? $raw->toHtml() : 'x';   // type of '$raw' could not be resolved
```

The boolean carries the assertion, so a truthy check on it should apply
the same narrowing as the original expression.

#### B52. `isset()` in a short-circuit condition does not mark the variable defined

**Impact: Medium · Effort: Low-Medium**

The right-hand side of `isset($x) &&` only evaluates when `$x` exists,
and the right-hand side of `!isset($x) ||` likewise. Both still report
the variable as undefined:

```blade
@if (isset($isOutlet) && $isOutlet == 1)          {{-- Undefined variable '$isOutlet' --}}
@if (!isset($stockGtr0) || $stockGtr0 == 'true')  {{-- Undefined variable '$stockGtr0' --}}
```

4 diagnostics in one sample project. This is the definite-vs-possible
existence question from
[T29](type-inference.md#t29-definite-vs-possible-variable-existence-tracking),
narrowed to the one shape that shows up constantly in templates.

#### B53. `$this->mock(Foo::class)` loses the intersection with `Foo`

**Impact: Low-Medium · Effort: Low**

Laravel declares `InteractsWithContainer::mock()` as returning
`MockInterface`; the useful type is `Foo&MockInterface`, which is what
callers annotate:

```php
private function mockHelloRetailClient(): Client&MockInterface
{
    $mock = $this->mock(Client::class);
    // Return type Mockery\MockInterface is incompatible with declared
    // return type Client&MockInterface
    return $mock;
}
```

Same treatment for `partialMock()`, `spy()` and `instance()`.

#### B54. A line comment plus an array-key assignment loses scope in a Blade `<?php` block

**Impact: Low · Effort: Medium**

Two ingredients that are individually harmless combine to drop a
`@var`-declared variable from scope for the rest of the block: a `//`
(or `#`) line comment inside the raw PHP region, and an array-key
assignment before the failing statement. Minimal reproduction:

```blade
@php
    /**
     * @var App\ViewModels\ShowViewModel $model
     */
@endphp
<?php
// short
$schema = [];

if ($model->rawImageUrl !== null) {
    $schema['image'] = $model->rawImageUrl;
}

if ($model->ratingCount > 0) {
    $schema['aggregateRating'] = [
        'ratingValue' => $model->ratingScore,   // type of '$model' could not be resolved
    ];
}
?>
```

`$model->rawImageUrl` and `$model->ratingCount` resolve; `$model` inside
the nested array literal does not. Remove either the comment or the
`$schema['image']` assignment and it resolves. A `/* … */` block comment
in place of the line comment also resolves, which points at offset
mapping between the Blade source and the preprocessed PHP rather than at
the walker itself.
