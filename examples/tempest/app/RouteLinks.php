<?php

declare(strict_types=1);

namespace App;

use function Tempest\Router\uri;

final class RouteLinks
{
    /** @return list<string> */
    public function links(): array
    {
        return [
            // Try: complete book:, follow it to $book, or rename the parameter.
            uri([BookController::class, 'show'], book: 'php'),
            // Try: complete the decorated URI or follow it to show().
            uri('/admin/v1/books/{book}', book: 'php'),
            // Try: complete locale: or follow it to the invokable action's parameter.
            uri(HomeController::class, locale: 'en'),
            // Try: complete a controller name or follow it to __invoke().
            uri('App\\HomeController', locale: 'en'),
        ];
    }
}
