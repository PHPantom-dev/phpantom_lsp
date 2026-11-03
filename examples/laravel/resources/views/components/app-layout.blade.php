{{-- The page shell resources/theme/views/theme/dashboard.blade.php wraps
     itself in, as an anonymous component: `<x-app-layout>` resolves to this
     template because no class backs it. --}}

<main {{ $attributes }}>
    {{ $slot }}
</main>
