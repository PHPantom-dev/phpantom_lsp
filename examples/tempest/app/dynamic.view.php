<!-- Try: inspect $book in the selected component and $displayed in this slot. -->
<x-component :is="$component" :book="$book" :count="2">
    <p>{{ $displayed->summary() }}</p>
</x-component>
<!-- Try: follow this component name, or complete its declared book attribute. -->
<x-component :is="'x-dynamic-book'" :book="$book" :count="1" />
