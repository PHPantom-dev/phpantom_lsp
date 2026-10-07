{{-- Demonstrates completion and navigation in nested Blade views --}}
@php
/**
 * @bladestan-signature
 * @var \App\Models\AuthorCollection $users
 */
@endphp

@extends('welcome')

{{-- The name is checked against the layouts this view extends: Ctrl+Click
     it to reach the @yield in welcome.blade.php, and a name none of them
     render is reported. Try: type a name inside @section(''). --}}
@section('content')
    <h1>{{ __('messages.welcome') }} - Admin</h1>

    {{-- $posts is declared nowhere in this file and no `view()` call passes
         it. It comes from welcome.blade.php, the layout this view extends:
         Laravel renders a layout from the same data array as its child, so
         whatever the layout declares the child receives too. The whole
         chain contributes, and this view's own signature wins for a name
         both declare.
         Try: hover $posts, or type $posts-> for completion. --}}
    <p>{{ $posts->published()->count() }} posts in total</p>

    <table>
        <thead>
            <tr>
                <th>Name</th>
                <th>Email</th>
                <th>Badge</th>
            </tr>
        </thead>
        <tbody>
            @foreach($users->active()->byName() as $user)
                @php $rowLabel = 'Author: ' . $user->name; @endphp
                <tr>
                    <td>{{ $user->name }}</td>
                    <td>{{ $user->email }}</td>
                    {{-- Bound component attributes are real PHP, so $rowLabel
                         (used only here) is not "unused" and $user resolves
                         for hover/go-to-definition. The same `:attr` on a
                         plain HTML tag stays text, because Alpine and Vue
                         bind with that syntax too. --}}
                    <td><x-widgets::badge label="Author" :author="$user" :title="$rowLabel" /></td>
                </tr>
            @endforeach
        </tbody>
    </table>

    @if($users->isEmpty())
        <p>{{ trans('pagination.next') }}</p>
    @endif
@endsection

@push('scripts')
    <script>console.log('pushed onto the layout stack')</script>
@endpush
