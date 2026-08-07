<?php

namespace App\Policies;

use App\Models\Customer;
use App\Models\Review;

/**
 * Named by the `#[UsePolicy]` attribute on App\Models\Review, which is the
 * Laravel 11+ way to point a model at a policy whose name does not follow the
 * convention.
 */
class ReviewModerationPolicy
{
    public function moderate(Customer $user, Review $review): bool
    {
        return $user->isPremium();
    }
}
