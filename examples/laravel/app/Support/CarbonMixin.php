<?php

namespace App\Support;

use Carbon\CarbonInterface;

/**
 * A Carbon trait-based mixin: each public method becomes a method on the
 * Carbon class it is mixed into.  Unlike a class-based mixin (where each
 * method returns a closure), Carbon supports mixing in traits directly —
 * the trait's own method signatures become the macro signatures.
 *
 * Registered via `CarbonImmutable::mixin(CarbonMixin::class)` in
 * DemoServiceProvider::boot().
 *
 * @phpstan-ignore trait.unused (Carbon mixin)
 */
trait CarbonMixin
{
    public function toTz(string $tz, bool $shift = false): CarbonInterface
    {
        return $shift
            ? $this->shiftTimezone($tz)
            : $this->timezone($tz);
    }

    public function toAppTz(bool $shift = false): CarbonInterface
    {
        return $this->toTz(config('app.timezone'), $shift);
    }
}
