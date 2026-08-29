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
    $actual = $type ? (string) $type : 'mixed';
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
$masterRecipe = (new \App\Models\Bakery())->masterRecipe();
check(
    'Bakery::masterRecipe() uses the ingredient pivot accessor',
    $masterRecipe->getPivotAccessor() === 'ingredient'
);
check(
    'Bakery::masterRecipe() uses RecipeIngredient as its pivot model',
    $masterRecipe->getPivotClass() === \App\Models\RecipeIngredient::class
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

// #[UseEloquentBuilder] hands the query off to the custom builder, and the
// builder keeps a reference to the model it was built for — which is why the
// analyzer can name the concrete model at the end of the chain even though
// LoafBuilder declares no `@template` of its own.
$result = \App\Models\Loaf::query();
check(
    'Loaf::query() returns LoafBuilder (#[UseEloquentBuilder])',
    $result instanceof \App\Models\LoafBuilder
);
check(
    'LoafBuilder::stale() keeps the chain on LoafBuilder',
    $result->stale() instanceof \App\Models\LoafBuilder
);
check(
    'LoafBuilder holds the Loaf model it queries',
    $result->getModel() instanceof \App\Models\Loaf
);

$result = \App\Models\Baker::query();
check(
    'Baker::query() returns BakerBuilder (#[UseEloquentBuilder])',
    $result instanceof \App\Models\BakerBuilder
);
check(
    'BakerBuilder holds the Baker model it queries',
    $result->getModel() instanceof \App\Models\Baker
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

// ─── Storage::disk() / cloud() concrete adapter ──────────────────────────────

// disk()/cloud() declare the Filesystem/Cloud contract, but every driver
// config/filesystems.php configures ('local', 's3') builds a
// FilesystemAdapter. The analyzer only corrects the return type when it can
// confirm what every configured disk is built from: a framework-shipped
// driver, or a Storage::extend() closure whose return type it can read.
check(
    'FilesystemAdapter::download() exists',
    method_exists(\Illuminate\Filesystem\FilesystemAdapter::class, 'download')
);
check(
    'FilesystemAdapter implements the Cloud contract',
    is_subclass_of(
        \Illuminate\Filesystem\FilesystemAdapter::class,
        \Illuminate\Contracts\Filesystem\Cloud::class
    )
);
// The Cloud contract deliberately lacks download() — this is why the
// precise adapter return type matters for cloud() too.
check(
    'Cloud contract does NOT declare download()',
    !method_exists(\Illuminate\Contracts\Filesystem\Cloud::class, 'download')
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

// ─── Model factory dynamic methods (has*/for*/trashed) ───────────────────────

// Factory routes has{Rel}()/for{Rel}()/trashed() through __call — none of
// them are declared methods, which is why the analyzer must synthesize them.
check(
    'Factory uses __call (has*/for*/trashed are magic)',
    method_exists(\Illuminate\Database\Eloquent\Factories\Factory::class, '__call')
);
check(
    'Factory does NOT declare hasPosts()',
    !method_exists(\Illuminate\Database\Eloquent\Factories\Factory::class, 'hasPosts')
);

// Convention resolves App\Models\BlogAuthor → Database\Factories\BlogAuthorFactory
// without a generic annotation, even through the shared BaseFactory parent.
$authorFactory = \App\Models\BlogAuthor::factory();
check(
    'BlogAuthor::factory() resolves to BlogAuthorFactory by convention',
    $authorFactory instanceof \Database\Factories\BlogAuthorFactory
);
check(
    'BlogAuthorFactory inherits through the shared BaseFactory',
    is_subclass_of(
        \Database\Factories\BlogAuthorFactory::class,
        \Database\Factories\BaseFactory::class
    )
);
check(
    'BlogAuthor::factory()->makeOne() builds one BlogAuthor',
    $authorFactory->makeOne() instanceof \App\Models\BlogAuthor
);

// EditorialFactory cannot be paired by name. Laravel reads its explicit
// protected $model property before trying that convention.
$editorialFactory = \Database\Factories\EditorialFactory::new();
check(
    'EditorialFactory declares BlogAuthor through its model property',
    $editorialFactory->modelName() === \App\Models\BlogAuthor::class
);
check(
    'EditorialFactory::makeOne() builds one BlogAuthor',
    $editorialFactory->makeOne() instanceof \App\Models\BlogAuthor
);
check(
    'EditorialFactory count builds the BlogAuthor collection',
    $editorialFactory->count(2)->make() instanceof \App\Models\AuthorCollection
);

// has{Relationship} is valid because posts() is a real relationship, and it
// returns the factory so the chain continues into create()/make().
check(
    'BlogAuthor::posts() is a HasMany relationship',
    (new \App\Models\BlogAuthor())->posts() instanceof \Illuminate\Database\Eloquent\Relations\HasMany
);
check(
    'BlogAuthor::factory()->hasPosts(3) returns a Factory',
    $authorFactory->hasPosts(3) instanceof \Illuminate\Database\Eloquent\Factories\Factory
);

// for{Relationship} is valid because author() is a BelongsTo relationship.
check(
    'BlogPost::author() is a BelongsTo relationship',
    (new \App\Models\BlogPost())->author() instanceof \Illuminate\Database\Eloquent\Relations\BelongsTo
);
check(
    'BlogPost::factory()->forAuthor() returns a Factory',
    \App\Models\BlogPost::factory()->forAuthor() instanceof \Illuminate\Database\Eloquent\Factories\Factory
);

// trashed() is only synthesized when the model is soft-deletable.
check(
    'BlogPost uses SoftDeletes',
    in_array(
        \Illuminate\Database\Eloquent\SoftDeletes::class,
        class_uses_recursive(\App\Models\BlogPost::class),
        true
    )
);
check(
    'BlogAuthor is NOT soft-deletable (no trashed())',
    ! \App\Models\BlogAuthor::isSoftDeletable()
);
check(
    'BlogPost::factory()->trashed() returns a Factory',
    \App\Models\BlogPost::factory()->trashed() instanceof \Illuminate\Database\Eloquent\Factories\Factory
);

// AnnotatedPostFactory carries `@extends Factory<BlogPost>`. The generic
// binding resolves create()/make() on its own; forAuthor()/trashed() still
// come from BlogPost's relationships and SoftDeletes trait through __call().
$annotatedPostFactory = \Database\Factories\AnnotatedPostFactory::new();
check(
    'AnnotatedPostFactory::forAuthor() returns a Factory despite the @extends generic',
    $annotatedPostFactory->forAuthor() instanceof \Illuminate\Database\Eloquent\Factories\Factory
);
check(
    'AnnotatedPostFactory::trashed() returns a Factory despite the @extends generic',
    $annotatedPostFactory->trashed() instanceof \Illuminate\Database\Eloquent\Factories\Factory
);
check(
    'AnnotatedPostFactory::make() builds a BlogPost via the generic binding',
    $annotatedPostFactory->make() instanceof \App\Models\BlogPost
);

// ─── Factory count-conditional return types ──────────────────────────────────

// create()/make() hand back a single model until the chain sets a count, at
// which point they hand back a collection — the one the model itself builds,
// so BlogAuthor's #[CollectedBy(AuthorCollection::class)] applies here too.
check(
    'BlogAuthor::factory()->make() builds one model',
    \App\Models\BlogAuthor::factory()->make() instanceof \App\Models\BlogAuthor
);
check(
    'an array argument to factory() is state, not a count',
    \App\Models\BlogAuthor::factory(['name' => 'Ada'])->make() instanceof \App\Models\BlogAuthor
);
check(
    'BlogAuthor::factory(3)->make() builds the model collection',
    \App\Models\BlogAuthor::factory(3)->make() instanceof \App\Models\AuthorCollection
);
check(
    'BlogAuthor::factory()->count(2)->make() builds two models',
    \App\Models\BlogAuthor::factory()->count(2)->make()->count() === 2
);
check(
    'BlogAuthorFactory::times(2)->make() builds the model collection',
    \Database\Factories\BlogAuthorFactory::times(2)->make() instanceof \App\Models\AuthorCollection
);
check(
    'count(null) clears a count set through factory($count)',
    \App\Models\BlogAuthor::factory(3)->count(null)->make() instanceof \App\Models\BlogAuthor
);
check(
    'hasPosts() counts related models, not the ones being built',
    \App\Models\BlogAuthor::factory()->hasPosts(3)->make() instanceof \App\Models\BlogAuthor
);

// ─── Carbon macro closure scope binding ──────────────────────────────────────

// Carbon binds macro closures with the target class as scope, so `self::`
// inside the closure refers to CarbonImmutable (not the class that lexically
// encloses the registration) and the protected `Mixin::this()` helper — the
// instance the macro is called on — is accessible.
\Carbon\CarbonImmutable::macro('phpantomScopeProbe', function (): string {
    return self::this()->format('Y');
});
check(
    'self::this() inside a Carbon macro returns the bound instance',
    \Carbon\CarbonImmutable::create(2020, 1, 1)->phpantomScopeProbe() === '2020'
);
check(
    'Mixin::this() is protected static (only reachable via rebound scope)',
    (new ReflectionMethod(\Carbon\CarbonImmutable::class, 'this'))->isProtected()
);

// ─── Validation rules are the request's input contract ──────────────────────

// Demo::requestInputKeys() claims these keys complete inside
// `$request->input('…')`.  They come straight from rules(), so assert the
// rule set the demo comments describe is the one the class actually declares.
$bakeryRules = (new \App\Http\Requests\StoreBakeryRequest())->rules();
check(
    'StoreBakeryRequest::rules() declares the demoed keys',
    array_keys($bakeryRules) === [
        'name',
        'apricot',
        'dough_temp',
        'notes',
        'notes.*.body',
        'owner.email',
        'photo',
        'gallery',
        'gallery.*',
        'flavor',
        'batch_size',
    ]
);
check(
    'FormRequest extends Request, so its input accessors are inherited',
    is_subclass_of(
        \Illuminate\Foundation\Http\FormRequest::class,
        \Illuminate\Http\Request::class
    )
);
// `safe()` has no native return type — its docblock says
// `ValidatedInput|array` — so the demo's `safe()->only([…])` relies on
// ValidatedInput declaring the narrowing methods.
check(
    'ValidatedInput narrows with only()/except()',
    method_exists(\Illuminate\Support\ValidatedInput::class, 'only')
        && method_exists(\Illuminate\Support\ValidatedInput::class, 'except')
);

// ─── Validated arrays only carry keys the rules named ───────────────────────

// Demo::validatedArrayShape() reads StoreBakeryRequest's rules as an array
// shape.  Two claims it makes are worth pinning to the real validator: that
// the result carries only keys the rules named, and — the reason `apricot`
// and `dough_temp` are *optional* keys rather than nullable ones — that a
// field which is merely allowed is absent when it was not sent.
//
// Built directly rather than through the facade, which needs a booted
// container.
$translator = new \Illuminate\Translation\Translator(
    new \Illuminate\Translation\ArrayLoader(),
    'en'
);
$validated = (new \Illuminate\Validation\Validator(
    $translator,
    [
        'name' => 'Sourdough',
        'owner' => ['email' => 'baker@example.com'],
        'flavor' => 'strawberry',
        'batch_size' => 12,
        'unlisted' => 'ignored',
    ],
    (new \App\Http\Requests\StoreBakeryRequest())->rules()
))->validated();
check(
    'validated() drops input the rules do not name',
    ! array_key_exists('unlisted', $validated)
);
check(
    'validated() keeps the fields that were supplied',
    ($validated['name'] ?? null) === 'Sourdough'
);
check(
    'an unsent optional field is absent from validated()',
    ! array_key_exists('apricot', $validated)
);
// `dough_temp` is `nullable|numeric`.  `nullable` permits a null value; it
// does not make the key appear, which is why the shape marks it optional
// (`dough_temp?: ?int|float`) rather than merely nullable.
check(
    'an unsent nullable field is absent, not present-and-null',
    ! array_key_exists('dough_temp', $validated)
);
// An enum rule validates the raw input and hands it back unchanged, which is
// why the shape types `flavor` as `string` and `batch_size` as `int` rather
// than as the enum itself.
check(
    'an enum rule validates to the raw scalar, not the enum case',
    ($validated['flavor'] ?? null) === 'strawberry'
        && ($validated['batch_size'] ?? null) === 12
);
check(
    'the demoed enums are backed by the types the shape claims',
    (string) (new ReflectionEnum(\App\Models\JamFlavor::class))->getBackingType() === 'string'
        && (string) (new ReflectionEnum(\App\Models\BatchSize::class))->getBackingType() === 'int'
);

// ─── Composed rules and excluded fields ─────────────────────────────────────

// Demo::inheritedRequestInputKeys() claims a child request that writes
// `array_merge(parent::rules(), […])` carries both arrays' keys, and that an
// `exclude` rule keeps its field out of validated() while `exclude_if` only
// sometimes does.
$updateRules = (new \App\Http\Requests\UpdateBakeryRequest())->rules();
check(
    'UpdateBakeryRequest::rules() carries its own keys and the inherited ones',
    array_key_exists('slug', $updateRules)
        && array_key_exists('name', $updateRules)
        && array_key_exists('apricot', $updateRules)
);
$updateValidated = (new \Illuminate\Validation\Validator(
    $translator,
    [
        'slug' => 'sourdough',
        'confirm_slug' => 'sourdough',
        'reason' => 'renamed',
        'name' => 'Sourdough',
        'owner' => ['email' => 'baker@example.com'],
        'flavor' => 'strawberry',
        'batch_size' => 12,
    ],
    $updateRules
))->validated();
check(
    'an inherited rule still validates on the child request',
    ($updateValidated['name'] ?? null) === 'Sourdough'
);
check(
    'an `exclude` field is validated and then dropped',
    ! array_key_exists('confirm_slug', $updateValidated)
);
check(
    'an `exclude_if` field is kept when its condition does not hold',
    ($updateValidated['reason'] ?? null) === 'renamed'
);

// ─── Resource route URIs ────────────────────────────────────────────────────

// Route::resource() names no URI; the registrar derives one from the resource
// name, singularizing every segment into a {parameter}.  These assertions pin
// down the derivation the LSP reimplements, including the nested form used by
// routes/web.php and the ->parameters() override.
$resourceUris = static function (string $name, ?callable $configure = null): array {
    $router = new \Illuminate\Routing\Router(new \Illuminate\Events\Dispatcher());
    $registration = $router->resource($name, \App\Http\Controllers\BakeryController::class);
    if ($configure !== null) {
        $configure($registration);
    }
    $registration->register();

    $uris = [];
    foreach ($router->getRoutes() as $route) {
        $uris[$route->getName()] = $route->uri();
    }

    return $uris;
};

$photoUris = $resourceUris('photos');
check(
    'photos.show is photos/{photo}',
    ($photoUris['photos.show'] ?? null) === 'photos/{photo}'
);
check(
    'photos.edit is photos/{photo}/edit',
    ($photoUris['photos.edit'] ?? null) === 'photos/{photo}/edit'
);
check(
    'photos.create is photos/create',
    ($photoUris['photos.create'] ?? null) === 'photos/create'
);

$nestedUris = $resourceUris('bakeries.ovens');
check(
    'a nested resource singularizes each parent segment',
    ($nestedUris['bakeries.ovens.show'] ?? null) === 'bakeries/{bakery}/ovens/{oven}'
);
check(
    'the parent wildcard is kept on the collection route',
    ($nestedUris['bakeries.ovens.index'] ?? null) === 'bakeries/{bakery}/ovens'
);

$overriddenUris = $resourceUris('photos', static function ($registration): void {
    $registration->parameters(['photos' => 'grid']);
});
check(
    '->parameters() replaces the derived wildcard',
    ($overriddenUris['photos.show'] ?? null) === 'photos/{grid}'
);

$shallowUris = $resourceUris('bakeries.ovens', static function ($registration): void {
    $registration->shallow();
});
// Shallow member routes lose the parent segments from their *name* too.
check(
    '->shallow() drops the parent segments from the member routes',
    ($shallowUris['ovens.show'] ?? null) === 'ovens/{oven}'
);
check(
    '->shallow() leaves the collection routes nested',
    ($shallowUris['bakeries.ovens.index'] ?? null) === 'bakeries/{bakery}/ovens'
);

// A slash in the resource name is a URI prefix, not a name separator.
$prefixedUris = $resourceUris('bakeries/ovens');
check(
    'a slashed resource name becomes a URI prefix',
    ($prefixedUris['ovens.show'] ?? null) === 'bakeries/ovens/{oven}'
);

// ─── Resource registrations written as a chain link ─────────────────────────

// `ResourceRegistrar` builds its action from as/uses/middleware/where/missing,
// so a `->prefix()` on the registration's own chain is discarded while an
// `->as()` on the same chain reaches every generated name.  Registering on the
// router directly cannot express those, so they get their own helper.
$chainUris = static function (callable $build): array {
    $router = new \Illuminate\Routing\Router(new \Illuminate\Events\Dispatcher());
    $build($router)->register();

    $uris = [];
    foreach ($router->getRoutes() as $route) {
        $uris[$route->getName()] = $route->uri();
    }

    return $uris;
};

$controller = \App\Http\Controllers\BakeryController::class;

$chainPrefixed = $chainUris(
    static fn ($router) => $router->prefix('admin')->resource('photos', $controller)
);
check(
    'a chain prefix does not reach the resource URI',
    ($chainPrefixed['photos.show'] ?? null) === 'photos/{photo}'
);

$chainNamed = $chainUris(
    static fn ($router) => $router->as('admin')->resource('photos', $controller)
);
check(
    'a chain ->as() prefixes every generated route name',
    ($chainNamed['admin.photos.show'] ?? null) === 'photos/{photo}'
);

// The registrar appends its own separator, so the trailing dot people write
// out of habit doubles up rather than being absorbed.
$chainDotted = $chainUris(
    static fn ($router) => $router->name('admin.')->resource('photos', $controller)
);
check(
    'a trailing dot in a chain name prefix is not absorbed',
    isset($chainDotted['admin..photos.show'])
);

// `->as()` is replaced by a later one on the same chain rather than appended.
$chainReplaced = $chainUris(
    static fn ($router) => $router->as('a')->as('b')->resource('photos', $controller)
);
check(
    'the last ->as() on a chain wins',
    isset($chainReplaced['b.photos.show'])
);

// ─── Resource modifiers ─────────────────────────────────────────────────────

// getResourceMethods() intersects with only() and *then* subtracts except(),
// so the two combine instead of cancelling out.
$narrowed = $resourceUris('photos', static function ($registration): void {
    $registration->only(['index', 'create'])->except(['create']);
});
check(
    'only() and except() are both applied',
    array_keys($narrowed) === ['photos.index']
);

// An empty only() restricts to nothing, which is not the same as never
// having called it.
check(
    'an empty only() registers no routes',
    $resourceUris('photos', static function ($registration): void {
        $registration->only([]);
    }) === []
);

// apiResource() is expressed as an implicit only() of the five API methods,
// so an explicit only() replaces it and can bring `create` back.
$apiWidened = $chainUris(
    static fn ($router) => $router->apiResource('photos', $controller)->only(['create'])
);
check(
    'an explicit only() replaces the apiResource restriction',
    ($apiWidened['photos.create'] ?? null) === 'photos/create'
);

// shallow() takes an argument, so shallow(false) leaves a nested resource
// nested.
$notShallow = $resourceUris('bakeries.ovens', static function ($registration): void {
    $registration->shallow(false);
});
check(
    '->shallow(false) keeps the parent segments',
    ($notShallow['bakeries.ovens.show'] ?? null) === 'bakeries/{bakery}/ovens/{oven}'
);

// names() renames the resource every route is derived from; a per-method
// name() replaces one whole route name and skips the ->as() prefix.
$renamed = $resourceUris('photos', static function ($registration): void {
    $registration->names('images');
});
check(
    '->names() renames every generated route',
    ($renamed['images.show'] ?? null) === 'photos/{photo}'
);

$perMethod = $resourceUris('photos', static function ($registration): void {
    $registration->name('index', 'photos.list');
});
check(
    '->name($method, $name) replaces one whole route name',
    isset($perMethod['photos.list']) && !isset($perMethod['photos.index'])
);

// parameters() replaces the whole map while parameter() appends to it, so
// whichever came last on the chain applies.
$lastOverride = $resourceUris('photos', static function ($registration): void {
    $registration->parameters(['photos' => 'grid'])->parameter('photos', 'other');
});
check(
    'the last parameter override for a segment wins',
    ($lastOverride['photos.show'] ?? null) === 'photos/{other}'
);

// getResourceUri() deletes the last segment's wildcard from the nested URI
// without anchoring the deletion, so segments that singularize alike lose
// both wildcards.
$repeated = $resourceUris('company.companies');
check(
    'a repeated wildcard collapses the nested URI',
    ($repeated['company.companies.show'] ?? null) === 'company/companies/{company}'
);

// ─── Resource wildcard singularization ──────────────────────────────────────

// The wildcard is `Str::singular()` of the segment, which is Doctrine's
// inflector rather than a trailing-`s` rule.  These are the shapes a
// hand-rolled singularizer gets wrong.
$singulars = [
    'photos' => 'photo',
    'categories' => 'category',
    'addresses' => 'address',
    'leaves' => 'leaf',
    'cookies' => 'cookie',
    'viruses' => 'virus',
    'bonuses' => 'bonus',
    'heroes' => 'hero',
    'knives' => 'knife',
    'ties' => 'ty',
    'statuses' => 'status',
    'series' => 'series',
    'Photos' => 'Photo',
];
$wrong = [];
foreach ($singulars as $plural => $expected) {
    if (\Illuminate\Support\Str::singular($plural) !== $expected) {
        $wrong[] = $plural;
    }
}
check(
    'the resource wildcards we model match Str::singular()',
    $wrong === []
);

// ─── Router macros ──────────────────────────────────────────────────────────

// A macro body registers on the router the closure is bound to, so its routes
// belong to whichever file called the macro and inherit the group prefixes in
// force there.  This is how `laravel/ui` ships `Route::auth()`, and it is what
// the LSP reproduces when it walks a macro body from a route file.
\Illuminate\Routing\Router::macro('demoAuth', function (): void {
    $this->get('login', fn () => 'login')->name('login');
    $this->demoPasswordReset();
});
\Illuminate\Routing\Router::macro('demoPasswordReset', function (): void {
    $this->post('password/reset', fn () => 'reset')->name('password.update');
});

$macroRouter = new \Illuminate\Routing\Router(new \Illuminate\Events\Dispatcher());
$macroRouter->demoAuth();
$macroRouter->name('admin.')->prefix('admin')->group(static function ($router): void {
    $router->demoAuth();
});

$macroUris = [];
foreach ($macroRouter->getRoutes() as $route) {
    $macroUris[$route->getName()] = $route->uri();
}

check(
    'a macro body registers its routes on the router',
    ($macroUris['login'] ?? null) === 'login'
);
check(
    'a macro called from inside another macro registers its routes too',
    ($macroUris['password.update'] ?? null) === 'password/reset'
);
check(
    'the group prefixes at the call site reach the macro routes',
    ($macroUris['admin.login'] ?? null) === 'admin/login'
        && ($macroUris['admin.password.update'] ?? null) === 'admin/password/reset'
);

// ─── Route names built in a loop ────────────────────────────────────────────

// routes/web.php registers one route per entry of a literal array and names
// each of them by interpolation.  The LSP unrolls that loop statically, so the
// names and URIs it records have to be the ones the router really ends up
// with, including the order and the trimmed leading slash.
$loopRouter = new \Illuminate\Routing\Router(new \Illuminate\Events\Dispatcher());
$campaigns = ['black-friday' => ['perfume', 'skincare'], 'valentines' => ['gifts']];
foreach ($campaigns as $campaign => $sections) {
    $loopRouter->get("/{$campaign}", [$controller, 'index'])
        ->name("campaigns.{$campaign}.landing");

    foreach ($sections as $section) {
        $loopRouter->get("/{$campaign}/{$section}", [$controller, 'index'])
            ->name("campaigns.{$campaign}.{$section}");
    }
}

$loopUris = [];
foreach ($loopRouter->getRoutes() as $route) {
    $loopUris[$route->getName()] = $route->uri();
}
check(
    'a loop over a literal array names one route per entry',
    array_keys($loopUris) === [
        'campaigns.black-friday.landing',
        'campaigns.black-friday.perfume',
        'campaigns.black-friday.skincare',
        'campaigns.valentines.landing',
        'campaigns.valentines.gifts',
    ]
);
check(
    'an interpolated URI keeps the loop variables it was built from',
    ($loopUris['campaigns.black-friday.skincare'] ?? null) === 'black-friday/skincare'
);

// ─── Higher-order collection proxies ────────────────────────────────────────

// Every proxy the LSP knows how to type must actually be proxyable, and
// every proxyable method must be one the LSP knows how to type — otherwise
// `$reviews->somethingElse->x` would resolve against a proxy Laravel never
// creates, or a real proxy would fall through to `mixed`.
$proxiesProperty = new ReflectionProperty(
    \Illuminate\Support\Collection::class,
    'proxies'
);
$runtimeProxies = $proxiesProperty->getValue();
$typedProxies = [
    'average', 'avg', 'contains', 'doesntContain', 'each', 'every', 'filter',
    'first', 'flatMap', 'groupBy', 'hasMany', 'hasSole', 'keyBy', 'last',
    'map', 'max', 'min', 'partition', 'percentage', 'reject', 'skipUntil',
    'skipWhile', 'some', 'sortBy', 'sortByDesc', 'sum', 'takeUntil',
    'takeWhile', 'unique', 'unless', 'until', 'when',
];
sort($runtimeProxies);
sort($typedProxies);
check(
    'the LSP types exactly the collection methods Laravel proxies',
    $runtimeProxies === $typedProxies
);

// `map` collects the accessed member, so a proxy over a scalar member
// produces a collection of scalars rather than of the original items.
$proxied = new \Illuminate\Support\Collection([
    (object) ['rating' => 3],
    (object) ['rating' => 5],
]);
check(
    'map proxies the member access onto every item',
    $proxied->map->rating->all() === [3, 5]
);
check(
    'filter proxies the member as a predicate and keeps the items',
    $proxied->filter->rating->count() === 2
);
check(
    'sum proxies the member and returns its total',
    $proxied->sum->rating === 8
);
check(
    'first proxies the member as a predicate and returns one item',
    $proxied->first->rating->rating === 3
);
check(
    'contains proxies the member as a predicate and returns a bool',
    $proxied->contains->rating === true
);

// `sum` seeds its reduction with `0` and adds each member to it, so a
// nullable member still totals to a number.  This is why the LSP types
// `$reviews->sum->discount` as `float` rather than `?float`: reporting the
// null would flag correct code that passes the total to a `float` parameter.
$nullable = new \Illuminate\Support\Collection([
    (object) ['discount' => 1.5],
    (object) ['discount' => null],
]);
check(
    'sum over a nullable member is still a number',
    $nullable->sum->discount === 1.5
);

// `min` / `max` reduce with *no* initial value, so an empty collection has
// no extremum at all — which is why the LSP types them nullable even when
// the member itself is not.
check(
    'max over an empty collection is null',
    (new \Illuminate\Support\Collection())->max->rating === null
);
check(
    'min over an empty collection is null',
    (new \Illuminate\Support\Collection())->min->rating === null
);

// `Eloquent\Collection::map()` degrades to the base collection as soon as
// the mapped values stop being models — which is why the LSP types
// `$reviews->map->getTitle()` as `Support\Collection`, not `ReviewCollection`.
$models = new \App\Models\ReviewCollection([new \App\Models\Review()]);
check(
    'mapping an Eloquent collection to a scalar degrades to the base collection',
    $models->map->getTitle()::class === \Illuminate\Support\Collection::class
);
check(
    'filtering an Eloquent collection keeps the custom collection class',
    $models->filter->getTitle()::class === \App\Models\ReviewCollection::class
);

// `Eloquent\Collection` overrides `partition()` with an explicit `->toBase()`
// but does not override `groupBy()`, which keeps `Support\Collection`'s
// `static<…, static<…>>` annotation.  The two therefore differ, which is why
// the LSP degrades only one of them.
check(
    'grouping an Eloquent collection keeps the custom collection class',
    $models->groupBy->getTitle()::class === \App\Models\ReviewCollection::class
);
check(
    'partitioning an Eloquent collection degrades to the base collection',
    $models->partition->getTitle()::class === \Illuminate\Support\Collection::class
);
check(
    'a partitioned Eloquent collection still nests the custom collection',
    $models->partition->getTitle()->first()::class === \App\Models\ReviewCollection::class
);

// ─── Blade component scope ──────────────────────────────────────────────────

// Laravel puts two variables in scope of every component view that no caller
// passes: `$attributes` (the tag's attributes) and `$slot` (its body).  The
// LSP declares both in a component template, so their concrete classes have
// to be the ones the framework actually hands over.
$component = new class extends \Illuminate\View\Component {
    public function render(): string { return ''; }
};
$component->withAttributes(['class' => 'alert']);
check(
    '$attributes in a component view is a ComponentAttributeBag',
    $component->attributes::class === \Illuminate\View\ComponentAttributeBag::class
);
check(
    'ComponentAttributeBag::merge() returns another attribute bag',
    $component->attributes->merge(['role' => 'alert']) instanceof \Illuminate\View\ComponentAttributeBag
);

// `Factory::renderComponent()` builds `['slot' => new ComponentSlot(...)]`,
// so an empty component body still gives the template a real object.
$slot = new \Illuminate\View\ComponentSlot();
check('$slot in a component view is empty when the tag has no body', $slot->isEmpty());
check(
    'ComponentSlot renders its contents as HTML',
    (new \Illuminate\View\ComponentSlot('<b>hi</b>'))->toHtml() === '<b>hi</b>'
);

// `AnonymousComponent::data()` merges every tag attribute into the view
// data, whether or not `@props` names it — this is the runtime fact the
// LSP's call-site inference for `<x-…>` tags relies on: a component with
// no `@props` at all still receives every attribute as a variable.
$anonymous = new \Illuminate\View\AnonymousComponent('components.alert', ['messages' => ['hi']]);
$anonymous->withAttributes(['class' => 'mt-4']);
check(
    'AnonymousComponent::data() exposes every attribute, @props or not',
    array_key_exists('class', $anonymous->data()) && array_key_exists('messages', $anonymous->data())
);

// A provider that keeps its container key in a static property on the base
// class it extends reads that key through late static binding, so the value
// the subclass registers under is the one declared up the parent chain.
check(
    'static::$abstract on a subclass reads the base provider\'s declared key',
    \App\Providers\DemoServiceProvider::$abstract === 'pastry.oven'
        && \App\Providers\BaseDemoServiceProvider::$abstract === 'pastry.oven'
);

// `alias($abstract, $alias)` registers the *second* argument as the new name,
// the reverse of `bind()`, and `make()` follows the alias before it looks at
// the bindings.
$container = new \Illuminate\Container\Container();
$container->singleton('pastry.oven', fn () => new \App\Support\BakeryService());
$container->alias('pastry.oven', 'pastry.oven.alias');
check(
    'alias() names its second argument, which make() follows to the binding',
    $container->make('pastry.oven.alias') instanceof \App\Support\BakeryService
);

// Two providers binding one key: each `register()` replaces what the key held,
// so the key ends up with whatever the provider registered last put there.
// That is how an application swaps a default implementation out, and it is the
// answer PHPantom has to give for `app('pastry.oven')`.
$providers = new \Illuminate\Container\Container();
foreach (require __DIR__ . '/bootstrap/providers.php' as $provider) {
    if (is_subclass_of($provider, \App\Providers\BaseDemoServiceProvider::class)
        || $provider === \App\Providers\BaseDemoServiceProvider::class
    ) {
        (new $provider($providers))->register();
    }
}
check(
    'the provider registered last decides a key two providers bind',
    $providers->make('pastry.oven') instanceof \App\Support\BakeryService
);

// `Application::register()` applies a provider's `$bindings` / `$singletons`
// arrays itself, *after* calling `register()`, so a key in one of the arrays
// binds like any other and beats what `register()` bound it to.
$app = new \Illuminate\Foundation\Application(__DIR__);
$app->register(\App\Providers\DemoServiceProvider::class);
check(
    'a $bindings entry binds its key',
    $app->make('pastry.counter') instanceof \App\Support\PastryCounter
);
check(
    'a $singletons entry binds its key',
    $app->make('pastry.plain-oven') instanceof \App\Support\PlainOven
);
check(
    'a factory that declares a return type hands back that class',
    $app->make('pastry.tally') instanceof \App\Support\PastryCounter
);

// The `#[Signature]` attribute (Laravel 13) is read by the Command
// constructor exactly like a `$signature` property, so the attribute is the
// command's effective name and parameter list.
$prune = new \App\Console\Commands\PruneStaleLoavesCommand();
check(
    '#[Signature] declares the command name',
    $prune->getName() === 'bakery:prune-stale'
);
check(
    '#[Signature] declares the command options',
    $prune->getDefinition()->hasOption('days')
        && $prune->getDefinition()->getOption('days')->getDefault() === '7'
);

// `Facade::__callStatic()` forwards to whatever the accessor resolves to, so
// every public instance method of that class is reachable as a static call on
// the facade even though the facade declares none of them.  This is what lets
// PHPantom list the concrete class's members on a facade that never had a
// `@method static` docblock generated.
\App\Facades\Oven::swap(new \App\Support\BakeryService());
check(
    'a facade forwards a static call to a public instance method of its accessor class',
    \App\Facades\Oven::bake('sourdough') === 'a fresh sourdough'
);
check(
    'a `static` return on the concrete class hands back the concrete instance',
    \App\Facades\Oven::heatedTo('hot') instanceof \App\Support\BakeryService
);
// An accessor that returns a container binding key forwards exactly the same
// way: `__callStatic()` asks the container for the key and calls the instance
// it hands back.  Nothing in the facade names the class, which is why PHPantom
// has to go through the binding to know what its members are.
\App\Facades\PastryOven::swap(new \App\Support\BakeryService());
check(
    'a facade whose accessor is a container binding key forwards to the bound instance',
    \App\Facades\PastryOven::bake('brioche') === 'a fresh brioche'
);
\Illuminate\Support\Facades\Facade::clearResolvedInstances();

// ─── Component class members reaching the view ──────────────────────────────

// Blade renders a class component's view with the data `Component::data()`
// returns, which is every public property plus every public method. An
// argument-less method arrives as an InvokableComponentVariable (so the view
// can both print it and call it) and one that takes an argument as a plain
// Closure, which is why only the argument-less ones are variables a template
// can read. Framework members declared on Component itself are ignored.
$summary = new \App\View\Components\PostSummary(new \App\Models\BlogPost());
$viewData = $summary->data();
check(
    'a public property reaches the component view',
    array_key_exists('post', $viewData) && array_key_exists('heading', $viewData)
);
check(
    'an argument-less public method reaches the view as an invokable variable',
    ($viewData['wordCount'] ?? null) instanceof \Illuminate\View\InvokableComponentVariable
);
check(
    'a method that takes an argument is a bare closure, not a readable variable',
    ($viewData['excerpt'] ?? null) instanceof \Closure
);
check(
    'framework members of Component are not view data',
    !array_key_exists('render', $viewData) && !array_key_exists('data', $viewData)
);

// ─── Index components ───────────────────────────────────────────────────────

// `<x-card>` names a directory rather than a class. Blade's tag compiler
// falls back to the class inside that directory which repeats its name, so
// the tag reaches App\View\Components\Card\Card. Nothing in the tag name
// says so, which is why the component namespaces have to be indexed from
// the filesystem rather than guessed from the name.
// `componentClass()` itself needs a booted application, so the rule is
// checked one piece at a time: the class name the tag formats to, the fact
// that it names nothing, and the `$class . '\' . class_basename($class)`
// fallback the compiler tries next.
$compiler = new \Illuminate\View\Compilers\ComponentTagCompiler();
check(
    'a tag name formats to a studly class path',
    $compiler->formatClassName('card') === 'Card'
        && $compiler->formatClassName('forms.date-picker') === 'Forms\\DatePicker'
);
$guessed = 'App\\View\\Components\\' . $compiler->formatClassName('card');
check(
    'a bare directory name is not itself a component class',
    !class_exists($guessed)
);
check(
    'the class repeating its directory name is what the tag reaches',
    class_exists($guessed . '\\' . \Illuminate\Support\Str::afterLast($guessed, '\\'))
        && $guessed . '\\Card' === \App\View\Components\Card\Card::class
);

// ─── Shared data and view composers ─────────────────────────────────────────

// `View::composer('partials.*', …)` matches view names with `Str::is()`, so a
// `*` stands for any run of characters and nothing else is a wildcard.  This is
// the rule PHPantom applies when deciding which templates a composer reaches.
check(
    'a composer pattern matches view names through Str::is()',
    \Illuminate\Support\Str::is('partials.*', 'partials.sidebar')
        && \Illuminate\Support\Str::is('*', 'partials.sidebar')
        && !\Illuminate\Support\Str::is('partials.*', 'emails.blog_published')
        && !\Illuminate\Support\Str::is('profile', 'profiles')
);

// A composer body writes its data with `$view->with()`, which takes either a
// key and a value or a whole array, and returns the view so the calls chain.
// PHPantom reads both forms, and follows the chain.
$withParameters = (new ReflectionMethod(\Illuminate\View\View::class, 'with'))->getParameters();
check(
    'View::with() takes a key-or-array first argument and an optional value',
    count($withParameters) === 2
        && $withParameters[0]->getName() === 'key'
        && $withParameters[1]->isOptional()
);

// Shared data is merged *under* the view's own data, so a variable a caller
// passes wins over one `View::share()` registered.  A composer runs after both
// and writes straight into the view's data, so its value wins over either.
check(
    'the view factory keeps shared data in its own bag',
    method_exists(\Illuminate\View\Factory::class, 'share')
        && method_exists(\Illuminate\View\Factory::class, 'getShared')
);

// ─── Mailable render sites ───────────────────────────────────────────────────

// A mailable names its template through the Content its content() returns,
// whose data argument is named `with:` rather than sitting after the view
// name.  PHPantom pairs the two by name, so the constructor's parameter names
// are what the demo in app/Mail/OrderShipped.php depends on.
$contentParameters = array_map(
    fn (ReflectionParameter $p) => $p->getName(),
    (new ReflectionMethod(\Illuminate\Mail\Mailables\Content::class, '__construct'))->getParameters()
);
check(
    'Content names its template in view/html/text/markdown and its data in with',
    array_slice($contentParameters, 0, 5) === ['view', 'html', 'text', 'markdown', 'with']
);

// app/Mail/BlogPublished.php passes nothing to emails.blog_published, yet the
// template declares $post and $author: a mailable merges every public property
// it declares into the view data, skipping the ones Mailable itself declares.
$blogPublished = new \App\Mail\BlogPublished(
    new \App\Models\BlogPost(),
    new \App\Models\BlogAuthor()
);
// `buildViewData()` is protected; since PHP 8.1 reflection reaches it without
// being asked to.
$buildViewData = new ReflectionMethod(\Illuminate\Mail\Mailable::class, 'buildViewData');
$viewData = $buildViewData->invoke($blogPublished);
check(
    'a mailable hands its view the public properties it declares',
    array_key_exists('post', $viewData) && array_key_exists('author', $viewData)
);
check(
    'the properties Mailable itself declares stay out of the view data',
    !array_key_exists('callbacks', $viewData) && !array_key_exists('subject', $viewData)
);

// ─── Data arguments that are not arrays ──────────────────────────────────────

// The view factory accepts an Arrayable where a data array goes and converts
// it with toArray() before rendering, so `view('welcome', new WelcomeData())`
// hands the template what that method returns.  PHPantom reads the same
// return shape, which is only correct because the conversion happens.
$factory = (new ReflectionClass(\Illuminate\View\Factory::class))->newInstanceWithoutConstructor();
$parseData = new ReflectionMethod(\Illuminate\View\Factory::class, 'parseData');
$arrayable = new class implements \Illuminate\Contracts\Support\Arrayable {
    public function toArray(): array
    {
        return ['user' => null, 'posts' => []];
    }
};
check(
    'the view factory converts an Arrayable data argument with toArray()',
    $parseData->invoke($factory, $arrayable) === ['user' => null, 'posts' => []]
);

// ─── The scope an @include inherits ──────────────────────────────────────────

// @include compiles to a render of the partial with the variables defined at
// that point in the including template, so a partial's declared variable can
// arrive from the surrounding scope with no data passed at all — and with
// whatever type that scope holds, which is what PHPantom checks it against.
$compiler = new \Illuminate\View\Compilers\BladeCompiler(
    new \Illuminate\Filesystem\Filesystem(),
    sys_get_temp_dir()
);
check(
    '@include hands the partial the variables the including template holds',
    str_contains(
        $compiler->compileString("@include('partials.author_badge')"),
        'get_defined_vars()'
    )
);

// ─── @each item and key variables ────────────────────────────────────────────

// `@each('partial', $rows, 'row')` compiles to Factory::renderEach(), which
// renders the partial once per entry with only two variables in scope: the
// entry, under the name the third argument spells, and $key.  PHPantom types
// both from the collection, so a partial declaring them is checked against
// what the directive actually iterates.
$renderEach = new ReflectionMethod(\Illuminate\View\Factory::class, 'renderEach');
check(
    'Factory::renderEach() names the iteration variable in its third argument',
    array_map(
        fn (ReflectionParameter $p) => $p->getName(),
        $renderEach->getParameters()
    ) === ['view', 'data', 'iterator', 'empty']
);

// The item's type is the collection's element type and $key's is its key
// type, exactly as a foreach over the same collection binds them.
$collection = new \App\Models\PostCollection([new \App\Models\BlogPost()]);
foreach ($collection as $key => $post) {
    check(
        'iterating a PostCollection yields BlogPost entries',
        $post instanceof \App\Models\BlogPost
    );
    check(
        'a PostCollection entry key is an array key',
        is_int($key) || is_string($key)
    );
}

// ─── Authorization abilities and policies ───────────────────────────────────

// PHPantom resolves a model's policy without booting the app, so the order it
// checks — explicit registration, then `#[UsePolicy]`, then the naming
// convention — has to be the order the Gate itself uses.
$gate = new \Illuminate\Auth\Access\Gate(
    new \Illuminate\Container\Container(),
    fn () => null
);

check(
    'the naming convention finds a policy with no registration',
    $gate->getPolicyFor(\App\Models\Bakery::class) instanceof \App\Policies\BakeryPolicy
);
check(
    'the #[UsePolicy] attribute names the policy for a model',
    $gate->getPolicyFor(\App\Models\Review::class) instanceof \App\Policies\ReviewModerationPolicy
);

$gate->policy(\App\Models\BlogPost::class, \App\Policies\PublishingPolicy::class);
check(
    'an explicit registration wins over the naming convention',
    $gate->getPolicyFor(\App\Models\BlogPost::class) instanceof \App\Policies\PublishingPolicy
);

$gate->define('manage-bakery-network', fn () => true);
check('Gate::define() registers an ability by name', $gate->has('manage-bakery-network'));

// `Gate::resource()` expands to one ability per CRUD verb, which is the set
// PHPantom synthesizes for the shorthand.
$gate->resource('photos', \App\Policies\BakeryPolicy::class);
check(
    'Gate::resource() registers one ability per CRUD verb',
    $gate->has([
        'photos.viewAny',
        'photos.view',
        'photos.create',
        'photos.update',
        'photos.delete',
    ])
);

// ─── URL helper conditional return ──────────────────────────────────────────

$previousContainer = \Illuminate\Container\Container::getInstance();
$helperContainer = new \Illuminate\Container\Container();
$urlGenerator = new \Illuminate\Routing\UrlGenerator(
    new \Illuminate\Routing\RouteCollection(),
    \Illuminate\Http\Request::create('https://example.test')
);
$helperContainer->instance(
    \Illuminate\Contracts\Routing\UrlGenerator::class,
    $urlGenerator
);
\Illuminate\Container\Container::setInstance($helperContainer);

check('url() returns the URL generator', url() === $urlGenerator);
check('url(null) returns the URL generator', url(null) === $urlGenerator);
check("url('/login') returns a string", is_string(url('/login')));

\Illuminate\Container\Container::setInstance($previousContainer);

// ─── Summary ────────────────────────────────────────────────────────────────

echo "\n";
if ($failed === 0) {
    echo "\033[32m✓ All $passed assertions passed.\033[0m\n";
} else {
    echo "\033[31m✗ $failed failed, $passed passed.\033[0m\n";
    exit(1);
}
