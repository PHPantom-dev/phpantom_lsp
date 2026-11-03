<?php

use App\Http\Middleware\EnsureBakeryIsOpen;
use Illuminate\Foundation\Application;
use Illuminate\Foundation\Configuration\Exceptions;
use Illuminate\Foundation\Configuration\Middleware;

/*
 * Laravel 11+ bootstrap.  `withCommands()` registers Artisan commands from
 * directories outside the conventional `app/Console/Commands`, so the
 * commands under `app/Actions` are real Artisan commands even though nothing
 * about their name or location says so.  `withMiddleware()` registers the
 * middleware aliases and groups routes name, next to the framework's own.
 */
return Application::configure(basePath: dirname(__DIR__))
    ->withRouting(
        web: __DIR__.'/../routes/web.php',
    )
    ->withCommands([
        __DIR__.'/../app/Actions',
    ])
    ->withMiddleware(function (Middleware $middleware) {
        $middleware->alias([
            'bakery.open' => EnsureBakeryIsOpen::class,
        ]);
        $middleware->group('storefront', [
            'web',
            'bakery.open',
        ]);
    })
    ->withExceptions(function (Exceptions $exceptions) {
        //
    })->create();
