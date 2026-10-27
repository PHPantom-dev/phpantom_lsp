<?php

declare(strict_types=1);

namespace App\Blog;

final readonly class Post
{
    public function __construct(
        public string $slug,
        public string $title,
        public string $body,
    ) {}
}
