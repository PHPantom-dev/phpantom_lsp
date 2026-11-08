<?php

declare(strict_types=1);

namespace App\Catalog;

use Yiisoft\Arrays\ArrayHelper;
use Yiisoft\Db\Connection\ConnectionInterface;
use Yiisoft\Db\Query\Query;

/**
 * Reads the catalog through the Active Records in this directory.
 */
final class BookCatalog
{
    /**
     * Try: hover $book. The query is built for Book, so one() returns a
     * Book. Go to the definition of 'author' in with(), which lands on
     * Book::getAuthorQuery(). Clear it to complete the relations Book
     * declares, or misspell it to see it reported.
     */
    public function find(int $id): ?Book
    {
        $book = Book::query()->with('author')->where(['id' => $id])->one();
        return $book instanceof Book ? $book : null;
    }

    /**
     * Try: 'books.author' is a path of relations. Go to the definition of
     * 'books', which lands on its arm in Author::relationQuery(), and of
     * 'author', a relation of Book, the record 'books' is for. Type a dot
     * after 'author' to complete the relations of Author again.
     *
     * @return array<Author|array<string, mixed>>
     */
    public function authorsWithBooks(): array
    {
        return Author::query()->with('books.author')->orderBy(['name' => SORT_ASC])->all();
    }

    /**
     * Try: go to the definition of 'title' in get(), which lands on
     * Book::$title, or rename the property and watch the string follow.
     * Clear it to complete the record's attributes, or misspell it to see
     * it reported. relation('author') goes to the getter as with() does.
     */
    public function describe(Book $book): string
    {
        $author = $book->relation('author');
        $name = $author instanceof Author ? $author->name : 'unknown';
        return $book->title . ' by ' . $name . ' (' . $book->get('title') . ')';
    }

    /**
     * Try: Review declares no properties, so its attributes are the columns
     * of its table. Go to the definition of 'rating', which lands on the
     * column in src/Migration/M251108000000CreateCatalog.php, or hover it
     * for its type. Clear it to complete the columns, or misspell it to see
     * it reported.
     */
    public function stars(Review $review): string
    {
        $rating = $review->get('rating');
        return str_repeat('*', is_int($rating) ? $rating : 0);
    }

    /**
     * Try: hover $title. getValueByPath() walks 'book.title' into the entry's
     * Book, so it is a string. Go to the definition of 'title', which lands
     * on Book::$title, or clear the path to complete the entry's keys, and
     * type a dot after 'book' to complete Book's properties.
     *
     * @param array{book: Book, shelf: string} $entry
     */
    public function shelvedTitle(array $entry): string
    {
        $title = ArrayHelper::getValueByPath($entry, 'book.title');
        return $title . ' on shelf ' . $entry['shelf'];
    }

    /**
     * Try: the query names its tables and columns as strings. Go to the
     * definition of 'title' in 'b.title', which lands on its column in
     * src/Migration/M251108000000CreateCatalog.php, or of 'author', which
     * lands on the createTable() call. 'b' is the alias the query gives the
     * table Table::BOOK names, which is read from the constant, and the
     * migration names the table with the same constant. Hover a column for
     * its type, clear one to complete the columns of the tables the query
     * reads from, type 'a.' to complete those of author alone, or misspell
     * one to see it reported.
     *
     * @return list<mixed>
     */
    public function titlesBy(ConnectionInterface $db, string $author): array
    {
        return (new Query($db))
            ->select('b.title')
            ->from(Table::BOOK . ' b')
            ->innerJoin('author a', 'a.id = b.author_id')
            ->where(['a.name' => $author])
            ->orderBy(['b.title' => SORT_ASC])
            ->column();
    }
}
