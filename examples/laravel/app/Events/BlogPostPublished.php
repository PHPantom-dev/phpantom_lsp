<?php

namespace App\Events;

use App\Models\BlogPost;

/**
 * Try: the code lens above the class lists its listeners. NotifyBlogAuthor
 * is one because Laravel's event discovery reads the type hint on its
 * `handle()` method; nothing registers it.
 */
class BlogPostPublished
{
    public function __construct(public BlogPost $post)
    {
    }
}
