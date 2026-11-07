<!-- Try: complete :foreach and navigate from $book->summary(); its type comes from BookController. -->
<section :foreach="$books as $book">
    <!-- Try: complete x-card and :title, then jump from the tag to its view. -->
    <x-card :title="$book->title" />
    <p>{{ $book->summary() }}</p>
</section>
<p :forelse>No books yet.</p>
