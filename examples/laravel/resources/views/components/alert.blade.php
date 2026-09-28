{{-- An anonymous Blade component. Laravel puts two variables in scope of
     every component view that no caller passes:

     $attributes  Illuminate\View\ComponentAttributeBag — the attributes
                  the <x-alert ...> tag was written with
     $slot        Illuminate\View\ComponentSlot — the tag's body

     Try: hover $attributes and $slot, and trigger completion on
     `$attributes->` and `$slot->`. --}}

{{-- $messages is never listed in @props (this component declares none at
     all) — it comes straight from the `:messages="[...]"` attribute one
     of its callers passes, in welcome.blade.php. Every attribute a
     caller writes on the tag becomes a variable here, the same as
     Laravel's own AnonymousComponent::data() merges them into the view.
     Try: hover $messages. --}}

{{-- $title comes from a `<x-slot:title>` (or the legacy `<x-slot
     name="title">`) a caller nests inside the tag, in welcome.blade.php.
     It is declared here, in the component that receives it, never in the
     caller that writes the tag — the same as $slot, it arrives as a
     \Illuminate\View\ComponentSlot.
     Try: hover $title. --}}

{{-- A template-level import applies to the whole template, the same as
     it would in the compiled view. Try: Ctrl+Click OrderStatus below. --}}
@php
    use App\Models\OrderStatus;
@endphp

<div {{ $attributes->merge(['class' => 'alert', 'role' => 'alert']) }}>
    @isset($title)
        <h4>{{ $title }}</h4>
    @endisset

    @if ($slot->isEmpty())
        <em>{{ __('messages.welcome') }}</em>
    @else
        {{ $slot }}
    @endif

    <small>{{ OrderStatus::Completed->label() }}</small>

    @foreach ($messages ?? [] as $message)
        <p>{{ $message }}</p>
    @endforeach
</div>
