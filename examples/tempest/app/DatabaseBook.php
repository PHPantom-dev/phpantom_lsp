<?php

declare(strict_types=1);

namespace App;

use Tempest\Database\IsDatabaseModel;
use Tempest\Database\Table;

#[Table('books')]
final class DatabaseBook
{
    use IsDatabaseModel;

    public string $title;
    public string $isbn;
    public ?string $publishedAt = null;
}
