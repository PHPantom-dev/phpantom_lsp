<?php

declare(strict_types=1);

namespace App\Catalog;

use Yiisoft\ActiveRecord\ActiveQueryInterface;
use Yiisoft\ActiveRecord\ActiveRecord;

/**
 * An Active Record that declares its relations the way the package
 * recommends: as the arms of the match relationQuery() returns.
 */
final class Author extends ActiveRecord
{
    public int $id;
    public string $name;

    public function relationQuery(string $name): ActiveQueryInterface
    {
        return match ($name) {
            'books' => $this->hasMany(Book::class, ['author_id' => 'id']),
            default => parent::relationQuery($name),
        };
    }
}
