{{-- Demonstrates variable completion inside included partials --}}
@php
/**
 * @bladestan-signature
 * @var \App\Models\BlogPost $post
 * @var \App\Models\BlogAuthor $author
 */
@endphp

<div style="font-family: sans-serif; padding: 20px;">
    <h2>{{ __('messages.welcome') }}!</h2>

    <p>New post by {{ $author->name }}:</p>
    <h3>{{ $post->getTitle() }}</h3>
    <p>Slug: {{ $post->getSlug() }}</p>
    <p>Published: {{ $post->created_at->diffForHumans() }}</p>

    {{-- `publish` is a method of PublishingPolicy, which DemoServiceProvider
         registers for BlogPost, so the ability completes and hovers here the
         same way it does in PHP. --}}
    @can('publish', $post)
        <p>You can publish this post.</p>
    @endcan

    <footer>
        <p>&copy; {{ config('app.name') }} {{ date('Y') }}</p>
    </footer>
</div>
