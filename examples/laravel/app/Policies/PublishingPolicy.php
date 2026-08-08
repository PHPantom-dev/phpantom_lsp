<?php

namespace App\Policies;

use App\Models\BlogPost;
use App\Models\Customer;

/**
 * Bound to App\Models\BlogPost by the `Gate::policy()` call in
 * DemoServiceProvider, so the naming convention is never consulted for that
 * model — an explicit registration wins.
 */
class PublishingPolicy
{
    public function publish(Customer $user, BlogPost $post): bool
    {
        return $user->isPremium();
    }

    public function unpublish(Customer $user, BlogPost $post): bool
    {
        return $user->isPremium();
    }
}
