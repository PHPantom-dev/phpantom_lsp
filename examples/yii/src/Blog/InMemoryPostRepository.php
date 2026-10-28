<?php

declare(strict_types=1);

namespace App\Blog;

use Psr\Log\LoggerInterface;
use Psr\Log\NullLogger;

/**
 * The DI container builds this class for PostRepositoryInterface. Every key
 * of its definition in config/common/di/blog.php names something declared
 * here: a constructor parameter, a method, and a property.
 */
final class InMemoryPostRepository implements PostRepositoryInterface
{
    public int $pageSize = 10;

    private LoggerInterface $logger;

    /**
     * @param array<string, string> $posts Post bodies keyed by slug.
     */
    public function __construct(
        private readonly array $posts,
    ) {
        $this->logger = new NullLogger();
    }

    public function setLogger(LoggerInterface $logger): void
    {
        $this->logger = $logger;
    }

    public function latest(): array
    {
        $posts = [];
        foreach (array_slice($this->posts, 0, $this->pageSize, true) as $slug => $body) {
            $posts[] = $this->makePost($slug, $body);
        }
        return $posts;
    }

    public function findBySlug(string $slug): ?Post
    {
        if (!isset($this->posts[$slug])) {
            $this->logger->info('No post with the slug {slug}.', ['slug' => $slug]);
            return null;
        }
        return $this->makePost($slug, $this->posts[$slug]);
    }

    private function makePost(string $slug, string $body): Post
    {
        return new Post($slug, ucfirst(str_replace('-', ' ', $slug)), $body);
    }
}
