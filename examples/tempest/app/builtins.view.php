<!-- Try: hover the tag and directive names, or inspect their completion documentation. -->
<x-template :foreach="['Tempest'] as $name">
    <span :if="$name !== ''" :as="'strong'" :apply="['data-framework' => $name]">{{ $name }}</span>
    <span :else>Unnamed framework.</span>
</x-template>
<p :forelse>No frameworks.</p>
