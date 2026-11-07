<?php /** @var \App\Book $book */ ?>
<article>
    <!-- Try: $count is inferred as a string produced by the dynamic wrapper. -->
    <h2>{{ $book->title }}</h2>
    <span>{{ $count }}</span>
    <?php $displayed = $book; ?>
    <x-slot />
</article>
