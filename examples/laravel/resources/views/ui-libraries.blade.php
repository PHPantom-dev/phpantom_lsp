{{-- Components from Blade component libraries, read from the service
     providers of the packages that ship them. --}}
<div>
    {{-- Flux (livewire/flux). <flux:button> is the same component as
         <x-flux::button>, an anonymous component in the package's own
         views.

         Try: Ctrl+Click `flux:button` to open its template, type a
         space after `variant="primary"` to complete the attributes its
         @props declares, or type `<flux:` to list every Flux component. --}}
    <flux:button variant="primary">Save</flux:button>
    <x-flux::badge color="lime">New</x-flux::badge>

    {{-- Try: hover `flux:buton` to see it reported as an unknown component. --}}
    <flux:buton>Typo</flux:buton>

    {{-- Filament (filament/filament). Its packages register their views
         under the `filament` namespace, whose components/ directory
         <x-filament::…> tags name.

         Try: Ctrl+Click `x-filament::button`, or type `<x-filament::` to
         list Filament's components. --}}
    <x-filament::button color="danger">Delete</x-filament::button>
    <x-filament::badge color="success">Paid</x-filament::badge>
</div>
