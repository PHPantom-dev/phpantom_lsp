<?php

namespace App\Providers;

use App\Support\CollectionMixin;
use Illuminate\Support\Collection;
use Illuminate\Support\ServiceProvider;

class DemoServiceProvider extends ServiceProvider
{
    public function boot(): void
    {
        // A macro registered here becomes a real method on Collection:
        // it autocompletes, hovers with this signature, and type-checks.
        Collection::macro('sumField', function (string $field): float {
            return $this->sum($field);
        });

        // A mixin registers one macro per public method of the given object,
        // each taking the signature of the closure that method returns.
        // PHPantom recovers those from CollectionMixin's source.
        Collection::mixin(new CollectionMixin());
    }
}
