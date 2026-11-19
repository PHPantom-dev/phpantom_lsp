<?php

use App\Book;

/** @var list<Book> $books */
?>

<!-- Try: hover $book in the else branch; it belongs to the same loop. -->
<p :foreach="$books as $book" :if="$book->title !== ''">{{ $book->summary() }}</p>
<p :else>Untitled book.</p>
