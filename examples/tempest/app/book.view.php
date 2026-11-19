<!-- Try: complete or navigate the custom View's properties and methods through $this. -->
<h1>{{ $this->heading() }}</h1>
<p>{{ $this->book->summary() }}</p>

<!-- Try: $selected comes from BookView's data(), and the slot runs inside x-book. -->
<x-book :book="$selected">
    <x-slot name="heading"><h2>{{ $book->title }}</h2></x-slot>
    <!-- Try: $displayed is assigned by the component before its default slot. -->
    <p>{{ $displayed->summary() }}</p>
</x-book>
