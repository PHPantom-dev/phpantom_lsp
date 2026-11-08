<?php

declare(strict_types=1);

namespace App\Catalog;

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
}
