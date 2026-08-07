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
// without an @extends Factory<Model> generic on the factory.
$authorFactory = \App\Models\BlogAuthor::factory();
check(
    'BlogAuthor::factory() resolves to BlogAuthorFactory by convention',
    $authorFactory instanceof \Database\Factories\BlogAuthorFactory
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

// ─── Summary ────────────────────────────────────────────────────────────────

echo "\n";
if ($failed === 0) {
    echo "\033[32m✓ All $passed assertions passed.\033[0m\n";
} else {
    echo "\033[31m✗ $failed failed, $passed passed.\033[0m\n";
    exit(1);
}
