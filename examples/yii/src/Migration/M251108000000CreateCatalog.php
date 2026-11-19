<?php

declare(strict_types=1);

namespace App\Migration;

use App\Catalog\Table;
use Yiisoft\Db\Migration\MigrationBuilder;
use Yiisoft\Db\Migration\RevertibleMigrationInterface;

/**
 * Creates the tables of the Active Records in src/Catalog. Review declares
 * no properties, so its attributes are the columns created here. The author
 * and book tables are named by the constants of App\Catalog\Table.
 */
final class M251108000000CreateCatalog implements RevertibleMigrationInterface
{
    public function up(MigrationBuilder $b): void
    {
        $column = $b->columnBuilder();

        $b->createTable(Table::AUTHOR, [
            'id' => $column::primaryKey(),
            'name' => $column::string()->notNull(),
        ]);

        $b->createTable(Table::BOOK, [
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
        $b->dropTable(Table::BOOK);
        $b->dropTable(Table::AUTHOR);
    }
}
