<?php

declare(strict_types=1);

namespace App;

use Tempest\View\View;
use Tempest\Router\Get;

use function Tempest\View\view;

#[Admin]
final class BookController
{
    #[Get('/books')]
    public function index(): View
    {
        // Try: jump from this path to books.view.php, or complete its name.
        // Try: complete data names, follow them to their first template reads, or hover their types.
        return view(__DIR__ . '/books.view.php', books: [new Book('The PHP Book')], title: 'Books');
    }

    // Try: complete inside {book}, or follow it to the parameter below.
    // The method decorator runs first: /admin/v1/books/{book}.
    #[Versioned]
    #[ReadBook('books/{book}')]
    public function show(string $book): string
    {
        return $book;
    }
}
