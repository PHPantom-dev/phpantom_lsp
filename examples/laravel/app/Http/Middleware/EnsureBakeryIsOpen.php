<?php

namespace App\Http\Middleware;

use Closure;
use Illuminate\Http\Request;

/**
 * Registered as the `bakery.open` alias and the `storefront` group in
 * `bootstrap/app.php`.  Its `handle()` takes a parameter after the request
 * and the next closure, which a route passes as `bakery.open:sunday`.
 */
class EnsureBakeryIsOpen
{
    public function handle(Request $request, Closure $next, string $day = 'monday'): mixed
    {
        return $next($request);
    }
}
