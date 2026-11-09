<?php

declare(strict_types=1);

namespace App\Catalog;

use Yiisoft\ActiveRecord\ActiveRecord;

/**
 * An Active Record that declares no properties: its attributes are the
 * columns src/Migration creates in its table, which tableName() derives
 * from the class name, 'review'.
 */
#[\AllowDynamicProperties]
final class Review extends ActiveRecord
{
}
