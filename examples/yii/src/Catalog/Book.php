<?php

declare(strict_types=1);

namespace App\Catalog;

use Yiisoft\ActiveRecord\ActiveQueryInterface;
use Yiisoft\ActiveRecord\ActiveRecord;
use Yiisoft\ActiveRecord\Trait\MagicRelationsTrait;

/**
 * An Active Record that declares its relations with getters:
 * MagicRelationsTrait makes getAuthorQuery() the relation 'author'.
 */
final class Book extends ActiveRecord
{
    use MagicRelationsTrait;

    public int $id;
    public string $title;
    public int $author_id;

    public function getAuthorQuery(): ActiveQueryInterface
    {
        return $this->hasOne(Author::class, ['id' => 'author_id']);
    }
}
