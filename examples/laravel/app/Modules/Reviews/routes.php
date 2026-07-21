<?php

use Illuminate\Support\Facades\Route;

// This route file lives outside the conventional `routes/` directory.
// It is registered from RouteServiceProvider via
// `Route::middleware('web')->group(base_path('app/Modules/Reviews/routes.php'))`,
// so PHPantom must read that provider to discover these names.
Route::name('reviews.')
    ->prefix('reviews')
    ->group(function (): void {
        Route::get('/', fn () => 'index')->name('index');
        Route::post('/{review}', fn () => 'update')->name('update');
    });
