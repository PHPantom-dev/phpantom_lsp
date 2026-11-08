<?php

declare(strict_types=1);

namespace App\Migration;

use Yiisoft\Db\Migration\MigrationBuilder;
use Yiisoft\Db\Migration\RevertibleMigrationInterface;

/**
 * Creates the tables of the Active Records in src/Catalog. Review declares
 * no properties, so its attributes are the columns created here.
 */
final class M251108000000CreateCatalog implements RevertibleMigrationInterface
{
    public function up(MigrationBuilder $b): void
    {
        $column = $b->columnBuilder();

        $b->createTable('{{%author}}', [
            'id' => $column::primaryKey(),
            'name' => $column::string()->notNull(),
        ]);

        $b->createTable('{{%book}}', [
            'id' => $column::primaryKey(),
            'title' => $column::string()->notNull(),
            'author_id' => $column::integer()->notNull(),
        ]);

        $b->createTable('{{%review}}', [
            'id' => $column::primaryKey(),
            'book_id' => $column::integer()->notNull(),
            'rating' => $column::integer()->notNull(),
            'body' => $column::text(),
        ]);
    }

    public function down(MigrationBuilder $b): void
    {
        $b->dropTable('{{%review}}');
        $b->dropTable('{{%book}}');
        $b->dropTable('{{%author}}');
    }
}
