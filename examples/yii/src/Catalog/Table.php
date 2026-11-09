<?php

declare(strict_types=1);

namespace App\Catalog;

/**
 * The names of the catalog's tables, which the migration creates and the
 * queries in BookCatalog read from.
 */
final class Table
{
    public const AUTHOR = '{{%author}}';
    public const BOOK = '{{%book}}';
}
