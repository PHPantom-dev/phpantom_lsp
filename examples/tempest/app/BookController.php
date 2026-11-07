<?php

declare(strict_types=1);

namespace App;

use Tempest\View\View;

use function Tempest\View\view;

final class BookController
{
    public function index(): View
    {
        // Try: jump from this path to books.view.php, or complete its name.
        return view(__DIR__ . '/books.view.php', books: [new Book('The PHP Book')]);
    }
}
