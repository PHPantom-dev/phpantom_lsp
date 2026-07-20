<?php
/**
 * Laravel Demo Assertions
 *
 * Run: php examples/laravel/assertions.php
 *
 * These assertions verify that our assumptions about Laravel's runtime
 * behaviour are correct, so the LSP can model them accurately.
 * Uses only reflection (no database or app boot required).
 */

require_once __DIR__ . '/vendor/autoload.php';

// Boot Eloquent with an in-memory SQLite database
$capsule = new \Illuminate\Database\Capsule\Manager();
$capsule->addConnection([
    'driver'   => 'sqlite',
    'database' => ':memory:',
]);
$capsule->setAsGlobal();
$capsule->bootEloquent();

$passed = 0;
$failed = 0;

function check(string $label, bool $condition): void
{
    global $passed, $failed;
    if ($condition) {
        $passed++;
    } else {
        $failed++;
        echo "FAIL: $label\n";
    }
}

function assertMethodVisibility(string $class, string $method, string $expected): void
{
    $ref = new ReflectionMethod($class, $method);
    $actual = $ref->isPublic() ? 'public' : ($ref->isProtected() ? 'protected' : 'private');
    check("$class::$method() is $expected", $actual === $expected);
}

function assertMethodReturnType(string $class, string $method, string $expected): void
{
    $ref = new ReflectionMethod($class, $method);
    $type = $ref->getReturnType();
    $actual = $type ? $type->__toString() : 'mixed';
    check("$class::$method() returns $expected (got $actual)", $actual === $expected);
}

// ─── Scope vs Model method shadowing ────────────────────────────────────────

// Model::fresh() is public — a subclass CANNOT define a #[Scope] named "fresh"
// because PHP forbids changing the signature of an inherited public method.
// Our demo uses "freshlyBaked" instead.
check(
    'Model::fresh() exists',
    method_exists(\Illuminate\Database\Eloquent\Model::class, 'fresh')
);
assertMethodVisibility(\Illuminate\Database\Eloquent\Model::class, 'fresh', 'public');

// Our Bakery uses "freshlyBaked" to avoid the conflict
check(
    'Bakery::freshlyBaked() exists',
    method_exists(\App\Models\Bakery::class, 'freshlyBaked')
);
assertMethodVisibility(\App\Models\Bakery::class, 'freshlyBaked', 'protected');

// Verify #[Scope] attribute is present on freshlyBaked
$ref = new ReflectionMethod(\App\Models\Bakery::class, 'freshlyBaked');
$attrs = $ref->getAttributes(\Illuminate\Database\Eloquent\Attributes\Scope::class);
check('Bakery::freshlyBaked() has #[Scope] attribute', count($attrs) === 1);

// ─── Convention-based scopes ────────────────────────────────────────────────

// scopeXxx methods are public and accessible via __call as xxx()
check(
    'Bakery::scopeUnbaked() exists',
    method_exists(\App\Models\Bakery::class, 'scopeUnbaked')
);
assertMethodVisibility(\App\Models\Bakery::class, 'scopeUnbaked', 'public');

check(
    'Bakery::scopeTopping() exists',
    method_exists(\App\Models\Bakery::class, 'scopeTopping')
);
assertMethodVisibility(\App\Models\Bakery::class, 'scopeTopping', 'public');

// ─── Relationship methods ───────────────────────────────────────────────────

check(
    'Bakery::baguettes() exists',
    method_exists(\App\Models\Bakery::class, 'baguettes')
);
check(
    'Bakery::headBaker() exists',
    method_exists(\App\Models\Bakery::class, 'headBaker')
);
check(
    'Bakery::masterRecipe() exists',
    method_exists(\App\Models\Bakery::class, 'masterRecipe')
);

// ─── Accessor methods ───────────────────────────────────────────────────────

// Legacy accessor
check(
    'Bakery::getLoafNameAttribute() exists (legacy accessor)',
    method_exists(\App\Models\Bakery::class, 'getLoafNameAttribute')
);

// Modern Attribute accessor
check(
    'Bakery::sprinkle() exists (modern accessor)',
    method_exists(\App\Models\Bakery::class, 'sprinkle')
);

// ─── Runtime scope behaviour ────────────────────────────────────────────────

// Convention-based scopes via __call on instance return Builder
$bakery = new \App\Models\Bakery();
$result = $bakery->unbaked();
check(
    '$bakery->unbaked() returns Builder via __call',
    $result instanceof \Illuminate\Database\Eloquent\Builder
);

$result = $bakery->topping('choc');
check(
    '$bakery->topping("choc") returns Builder via __call',
    $result instanceof \Illuminate\Database\Eloquent\Builder
);

// #[Scope] attribute scopes are available on the query builder
$result = \App\Models\Bakery::query()->freshlyBaked();
check(
    'Bakery::query()->freshlyBaked() returns Builder',
    $result instanceof \Illuminate\Database\Eloquent\Builder
);

// Static scope forwarding
$result = \App\Models\Bakery::where('flour', 'rye');
check(
    'Bakery::where() returns Builder',
    $result instanceof \Illuminate\Database\Eloquent\Builder
);

// Model::fresh() on instance (non-existing model returns null)
$result = $bakery->fresh();
check(
    '$bakery->fresh() returns null (Model::fresh on non-persisted)',
    $result === null
);

// ─── Auth user model (config/auth.php) ───────────────────────────────────────

// The default `web` guard's provider model is App\Models\Customer and the
// `admin` guard's provider model is App\Models\Administrator, so the analyzer
// resolves Request::user() to Customer and auth('admin')->user() to
// Administrator.
$authConfig = require __DIR__ . '/config/auth.php';
check(
    'config/auth.php default guard is web',
    $authConfig['defaults']['guard'] === 'web'
);
check(
    'web guard provider model is Customer',
    $authConfig['providers'][$authConfig['guards']['web']['provider']]['model']
        === \App\Models\Customer::class
);
check(
    'admin guard provider model is Administrator',
    $authConfig['providers'][$authConfig['guards']['admin']['provider']]['model']
        === \App\Models\Administrator::class
);
check(
    'Customer is an Authenticatable',
    is_subclass_of(\App\Models\Customer::class, \Illuminate\Contracts\Auth\Authenticatable::class)
);
check(
    'Administrator is an Authenticatable',
    is_subclass_of(\App\Models\Administrator::class, \Illuminate\Contracts\Auth\Authenticatable::class)
);

// ─── Paginator element types ─────────────────────────────────────────────────

// paginate()/simplePaginate()/cursorPaginate() exist on the Eloquent Builder
// and the paginators they build are iterable, so a foreach over the result
// yields the model instances. The analyzer parameterises the return with
// <int, TModel> to recover the element type.
foreach (['paginate', 'simplePaginate', 'cursorPaginate'] as $m) {
    check(
        "Builder::$m() exists",
        method_exists(\Illuminate\Database\Eloquent\Builder::class, $m)
    );
}
check(
    'LengthAwarePaginator is iterable (IteratorAggregate)',
    is_subclass_of(\Illuminate\Pagination\LengthAwarePaginator::class, \IteratorAggregate::class)
);
check(
    'Paginator is iterable (IteratorAggregate)',
    is_subclass_of(\Illuminate\Pagination\Paginator::class, \IteratorAggregate::class)
);
check(
    'CursorPaginator is iterable (IteratorAggregate)',
    is_subclass_of(\Illuminate\Pagination\CursorPaginator::class, \IteratorAggregate::class)
);

// ─── Storage::fake() concrete adapter ────────────────────────────────────────

// fake() declares the Filesystem contract but always constructs a concrete
// FilesystemAdapter, which is where the test assertion helpers live. The
// analyzer corrects the return type to the adapter so these resolve.
check(
    'FilesystemAdapter implements the Filesystem contract',
    is_subclass_of(
        \Illuminate\Filesystem\FilesystemAdapter::class,
        \Illuminate\Contracts\Filesystem\Filesystem::class
    )
);
check(
    'FilesystemAdapter::assertExists() exists',
    method_exists(\Illuminate\Filesystem\FilesystemAdapter::class, 'assertExists')
);
check(
    'FilesystemAdapter::assertMissing() exists',
    method_exists(\Illuminate\Filesystem\FilesystemAdapter::class, 'assertMissing')
);
// The contract deliberately lacks the assertion helpers — this is why the
// precise adapter return type matters.
check(
    'Filesystem contract does NOT declare assertExists()',
    !method_exists(\Illuminate\Contracts\Filesystem\Filesystem::class, 'assertExists')
);

// ─── View contract → concrete binding ────────────────────────────────────────

// The object bound for the View contract is the concrete Illuminate\View\View,
// which uses the Macroable trait (and therefore has __call). The analyzer binds
// the concrete to the contract as a mixin so concrete-only methods resolve and
// macro calls no longer report as unknown.
check(
    'Concrete View uses Macroable (has __call)',
    method_exists(\Illuminate\View\View::class, '__call')
);
check(
    'Concrete View::getName() exists',
    method_exists(\Illuminate\View\View::class, 'getName')
);
check(
    'Concrete View::fragment() exists',
    method_exists(\Illuminate\View\View::class, 'fragment')
);
// The contract deliberately lacks these — this is why binding the concrete
// as a mixin on the contract matters.
check(
    'View contract does NOT declare getName()',
    !method_exists(\Illuminate\Contracts\View\View::class, 'getName')
);

// ─── Summary ────────────────────────────────────────────────────────────────

echo "\n";
if ($failed === 0) {
    echo "\033[32m✓ All $passed assertions passed.\033[0m\n";
} else {
    echo "\033[31m✗ $failed failed, $passed passed.\033[0m\n";
    exit(1);
}
