<?php

declare(strict_types=1);

namespace App;

use Tempest\View\View;

use function Tempest\View\view;

final class ConfiguredViews
{
    public const NOTICE = __DIR__ . '/../ui/notice.view.php';
    public const PAGE = __DIR__ . '/../resources/configured.view.php';

    public static function render(string $path, Book $book): View
    {
        return view($path, book: $book);
    }

    public function example(): View
    {
        $path = self::PAGE;

        // Try: inspect $book's inferred members in resources/configured.view.php.
        return self::render(book: new Book('Configured views'), path: $path);
    }
}
