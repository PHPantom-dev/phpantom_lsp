{{-- Try: Ctrl+Click on variables, config keys, route names, and translation keys --}}
@php
/**
 * @bladestan-signature
 * @var ?\App\Models\BlogAuthor $user
 * @var \App\Models\PostCollection $posts
 */
@endphp

<!DOCTYPE html>
<html lang="{{ config('app.locale') }}">
<head>
    <title>{{ config('app.name') }} - {{ __('messages.welcome') }}</title>
</head>
<body>
    <h1>{{ __('messages.welcome') }}</h1>

    {{-- Comment contents are inert: an apostrophe ('), a commented-out
         {{ $echo }}, and a mention of @endphp all stay inside the comment --}}

    {{-- Variable completion: $user-> triggers member suggestions --}}
    @if($user)
        <p>Hello, {{ $user->name }}!</p>
        <p>Email: {{ $user->email }}</p>
    @endif

    {{-- @error injects an implicit $message variable --}}
    @error('email')
        <div class="alert">{{ $message }}</div>
    @enderror

    {{-- @session injects an implicit $value variable --}}
    @session('status')
        <div class="success">{{ $value }}</div>
    @endsession

    {{-- @forelse with @empty — supported since recently --}}
    @forelse($posts->published()->byNewest() as $post)
        {{-- Standalone @var docblock for type narrowing in foreach --}}
        @php /** @var \App\Models\BlogPost $post */ @endphp
        <article>
            <h2>{{ $post->getTitle() }}</h2>
            <p>By {{ $post->author->name }}</p>
            <span>{{ $post->created_at->diffForHumans() }}</span>

            {{-- $loop variable is injected automatically in foreach/forelse --}}
            @if($loop->first)
                <span class="badge">Featured</span>
            @endif
            @if(!$loop->last)
                <hr>
            @endif
        </article>
    @empty
        <p>No posts found.</p>
    @endforelse

    {{-- Route and translation helpers (Ctrl+Click navigates to definition) --}}
    <nav>
        {{-- Go-to-definition works on view references in @include, @extends --}}
        <a href="{{ route('home') }}">{{ __('messages.welcome') }}</a>
        <a href="{{ route('admin.users.index') }}">{{ trans('auth.failed') }}</a>
    </nav>

    {{-- @include: Ctrl+Click navigates to the included view --}}
    @include('emails.order_shipped', ['post' => $posts->first()])

    {{-- The <x-alert> component view is where $attributes and $slot come
         from: resources/views/components/alert.blade.php --}}
    <x-alert class="mt-4">{{ __('messages.welcome') }}</x-alert>

    {{-- @verbatim: content inside is skipped by the preprocessor --}}
    @verbatim
        <p>This {{ $blade }} syntax is not processed</p>
    @endverbatim

    {{-- Conditional rendering with config --}}
    @if(config('app.debug'))
        <pre>Debug mode is on (env: {{ config('app.env') }})</pre>
    @endif

    @yield('content')
</body>
</html>
