<?php

declare(strict_types=1);

namespace App;

use Tempest\View\IsView;
use Tempest\View\View;

final class BookView implements View
{
    use IsView;

    public function __construct(public Book $book)
    {
        $this->path = __DIR__ . '/book.view.php';
        $this->data(selected: $book);
    }

    public function heading(): string
    {
        return $this->book->title;
    }
}
