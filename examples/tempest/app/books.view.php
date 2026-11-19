<h1>{{ $title }}</h1>
<!-- Try: complete :foreach and navigate from $book->summary(); its type comes from BookController. -->
<section :foreach="$books as $book">
    <!-- Try: follow x-card to its view, follow its attributes to their variables, and hover their types. -->
    <x-card :title="$book->title" :show-title="true" />
    <p>{{ $book->summary() }}</p>
</section>
<p :forelse>No books yet.</p>
