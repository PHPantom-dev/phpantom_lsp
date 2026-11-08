<?php

declare(strict_types=1);

use App\Blog\PostRepositoryInterface;
use App\Blog\PostViewed;
use App\Blog\ViewCounter;

return [
    // Try: go-to-definition on 'count', which opens ViewCounter::count().
    // Clear it to complete ViewCounter's public methods, rename count() in
    // src/Blog/ViewCounter.php and watch the string follow, or misspell it
    // to see it reported. Point it at 'viewsOf' to see it reported too:
    // its string $slug cannot be filled when the event is dispatched.
    PostViewed::class => [
        [ViewCounter::class, 'count'],
        // The event goes to the parameter its class fits, and the container
        // fills the rest. Drop the PostViewed type to see $event reported.
        static function (PostRepositoryInterface $posts, PostViewed $event): void {
            assert($posts->findBySlug($event->slug) !== null);
        },
    ],
];
