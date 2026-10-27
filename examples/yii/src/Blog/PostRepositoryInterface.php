<?php

declare(strict_types=1);

namespace App\Blog;

interface PostRepositoryInterface
{
    /**
     * @return list<Post>
     */
    public function latest(): array;

    public function findBySlug(string $slug): ?Post;
}
