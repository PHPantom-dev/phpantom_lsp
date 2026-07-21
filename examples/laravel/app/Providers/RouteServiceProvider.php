<?php

namespace App\Providers;

use Illuminate\Support\Facades\Route;
use Illuminate\Support\ServiceProvider;

class RouteServiceProvider extends ServiceProvider
{
    public function boot(): void
    {
        // Routes registered with the fluent `Route::…->group(base_path(…))`
        // API instead of `$this->loadRoutesFrom(…)`.  PHPantom scans this
        // registration so `route('reviews.update')` resolves even though the
        // route file lives under app/Modules, not the conventional routes/ dir.
        Route::middleware('web')
            ->group(base_path('app/Modules/Reviews/routes.php'));
    }
}
