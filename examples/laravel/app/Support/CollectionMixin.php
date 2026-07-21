<?php

namespace App\Support;

use Closure;
use Illuminate\Support\Collection;

/**
 * A Macroable mixin: every public method returns a closure that becomes a
 * macro on the class it is mixed into.  Registered via
 * `Collection::mixin(new CollectionMixin())` in DemoServiceProvider::boot().
 *
 * The returned closures are rebound to the target, so `$this` inside them is
 * the Collection.  The `@mixin` tag tells PHPantom that, so `$this->…` calls
 * resolve against Collection's members; PHPantom separately recovers each
 * method's returned-closure signature to register the macros on Collection.
 *
 * @mixin \Illuminate\Support\Collection
 */
class CollectionMixin
{
    /**
     * Registers a `toAssoc` macro: reduce the collection to an associative
     * array keyed by $keyField with $valueField as the value.
     */
    public function toAssoc(): Closure
    {
        return function (string $keyField, string $valueField): array {
            return $this->mapWithKeys(
                fn (array $item) => [$item[$keyField] => $item[$valueField]]
            )->all();
        };
    }
}
