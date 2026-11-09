<?php

declare(strict_types=1);

namespace App\Blog;

/**
 * Counts the views of each post. config/common/events.php attaches count()
 * to PostViewed, and the container builds the one instance it calls.
 */
final class ViewCounter
{
    /** @var array<string, int> */
    private array $views = [];

    public function count(PostViewed $event): void
    {
        $this->views[$event->slug] = $this->viewsOf($event->slug) + 1;
    }

    public function viewsOf(string $slug): int
    {
        return $this->views[$slug] ?? 0;
    }
}
