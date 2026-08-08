<?php

namespace App\Policies;

use App\Models\Bakery;
use App\Models\Customer;

/**
 * Found by Laravel's discovery convention: App\Models\Bakery looks for
 * App\Models\Policies\BakeryPolicy and then App\Policies\BakeryPolicy, so
 * this class governs Bakery without any registration.
 *
 * Every public method is an ability valid for that model, which is what
 * `Gate::allows('update', $bakery)` and `@can('update', $bakery)` name.
 */
class BakeryPolicy
{
    /** A hook that runs before every check — not an ability itself. */
    public function before(Customer $user, string $ability): ?bool
    {
        return null;
    }

    public function viewAny(Customer $user): bool
    {
        return true;
    }

    public function update(Customer $user, Bakery $bakery): bool
    {
        return $user->isPremium();
    }

    public function delete(Customer $user, Bakery $bakery): bool
    {
        return false;
    }

    /** Not public, so not an ability. */
    protected function sameOwner(Customer $user, Bakery $bakery): bool
    {
        return true;
    }
}
