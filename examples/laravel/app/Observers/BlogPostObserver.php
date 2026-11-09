<?php

namespace App\Observers;

use App\Models\BlogPost;

/**
 * Try: the code lens above the class goes back to BlogPost, whose
 * `#[ObservedBy]` attribute registers this observer.
 */
class BlogPostObserver
{
    public function created(BlogPost $post): void
    {
    }
}
