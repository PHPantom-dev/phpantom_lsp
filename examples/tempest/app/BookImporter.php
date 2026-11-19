<?php

declare(strict_types=1);

namespace App;

final class BookImporter
{
    public function __invoke(string $path, int $limit = 1): string
    {
        return self::import($path, $limit);
    }

    public static function import(string $path, int $limit = 1): string
    {
        return "{$path}:{$limit}";
    }
}
