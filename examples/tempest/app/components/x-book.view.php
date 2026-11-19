<article>
    <!-- Try: $book is inferred from the component attribute in book.view.php. -->
    <x-slot name="heading" />
    <?php $displayed = $book; ?>
    <x-slot />
</article>
