<?php

declare(strict_types=1);

namespace App\Blog;

/**
 * Dispatched when a post is read. Try: go-to-implementation on the class
 * name, or the lens above it, to find the listeners config/common/events.php
 * attaches to it.
 */
final readonly class PostViewed
{
    public function __construct(
        public string $slug,
    ) {}
}
