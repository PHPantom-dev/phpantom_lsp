<?php

namespace App\Listeners;

use App\Events\BlogPostPublished;

class NotifyBlogAuthor
{
    /**
     * Try: the code lens above this method goes to the event it handles.
     */
    public function handle(BlogPostPublished $event): void
    {
    }
}
