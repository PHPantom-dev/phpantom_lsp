<?php

namespace App;

use App\Models\Bakery;

/**
 * A plain (non-model) class that proxies to a Bakery via `@mixin`.
 *
 * `@mixin` exposes the whole public API of the target, including the
 * model's *virtual* members (relationship properties, scope methods,
 * cast-typed attributes) synthesized by PHPantom's Eloquent support, not
 * just its real declared members.
 *
 * @mixin Bakery
 */
class BakeryProxy
{
    public function __construct(private Bakery $bakery)
    {
    }

    public function __call(string $name, array $arguments): mixed
    {
        return $this->bakery->{$name}(...$arguments);
    }

    public function __get(string $name): mixed
    {
        return $this->bakery->{$name};
    }
}
