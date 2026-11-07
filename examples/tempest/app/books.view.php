<?php

use App\Book;

/** @var list<Book> $books */
?>

<!-- Try: complete :foreach and navigate from $book->summary() to Book.php. -->
<section :foreach="$books as $book">
    <!-- Try: complete x-card and :title, then jump from the tag to its view. -->
    <x-card :title="$book->title" />
    <p>{{ $book->summary() }}</p>
</section>
<p :forelse>No books yet.</p>
