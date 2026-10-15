<?php
use App\Http\Controllers\BakeryController;
use App\Http\Controllers\ResourceDemoController;
use App\Models\BlogAuthor;
use App\Models\BlogPost;
use Illuminate\Support\Facades\Route;

// welcome.blade.php declares $user and $posts, so a route closure rendering
// it has to hand over both.  Drop one and the call site is flagged.
Route::get('/', fn() => view('welcome', [
    'user' => BlogAuthor::first(),
    'posts' => BlogPost::all(),
]))->name('home');
Route::get('/bakeries', [BakeryController::class, 'index'])->name('bakeries.index');

// A macro registered in RouteServiceProvider.  The routes its body declares
// (login, logout, password.request, password.update) belong to this file, and
// the group prefixes in force at the call site reach them.
Route::bakeryAuth();

Route::prefix('admin')->group(function () {
    // admin/users/index.blade.php @extends('welcome'), and Laravel renders a
    // layout from the same data array as its child, so the layout's $user and
    // $posts belong to this call too.
    Route::get('/users', fn() => view('admin.users.index', [
        'users' => BlogAuthor::all(),
        'user' => BlogAuthor::first(),
        'posts' => BlogPost::all(),
    ]))->name('admin.users.index');
});

Route::prefix('bakeries')
    ->controller(BakeryController::class)
    ->group(function () {
        Route::get('{bakery}', 'show')->name('bakeries.show');
        Route::patch('{bakery}/cancel', 'cancel')->name('bakeries.cancel');
    });

// A resource registration names no URI: Laravel derives one from the resource
// name, singularizing each segment to build the {parameters}.  This nested
// name yields bakeries/{bakery}/ovens and bakeries/{bakery}/ovens/{oven}.
Route::resource('bakeries.ovens', BakeryController::class)->only(['index', 'show']);

// Plural registrations generate the usual routes for every array entry.
// Singletons keep the parent parameter but have no parameter of their own.
Route::name('catalog.')->prefix('catalog')->group(function () {
    Route::resources([
        'collections' => ResourceDemoController::class,
        'menus' => ResourceDemoController::class,
    ]);
    Route::apiResources([
        'deliveries' => ResourceDemoController::class,
        'orders' => ResourceDemoController::class,
    ]);
    Route::singleton('bakeries.profile', ResourceDemoController::class)->creatable();
    Route::apiSingleton('status', ResourceDemoController::class)->destroyable();
});

// A route file pulled in by path rather than by closure.  The path is held in
// a variable, which PHPantom follows back to the assignment, so the routes
// api/v1.php declares are found and inherit this group's URI prefix
// (api.v1.users.index is registered at api/v1/users).
$apiRoutes = __DIR__ . '/api/v1.php';

Route::group(['prefix' => 'api'], $apiRoutes);

// One route per entry of a literal array, each named by interpolation.  The
// array, the loop variables and the names they build are all statically
// known, so route('campaigns.black-friday.perfume') resolves here just as a
// written-out ->name() would.
$campaigns = ['black-friday' => ['perfume', 'skincare'], 'valentines' => ['gifts']];

foreach ($campaigns as $campaign => $sections) {
    Route::get("/{$campaign}", [BakeryController::class, 'index'])
        ->name("campaigns.{$campaign}.landing");

    foreach ($sections as $section) {
        Route::get("/{$campaign}/{$section}", [BakeryController::class, 'index'])
            ->name("campaigns.{$campaign}.{$section}");
    }
}
