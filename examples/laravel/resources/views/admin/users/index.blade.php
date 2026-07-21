{{-- Demonstrates completion and navigation in nested Blade views --}}
@php
/**
 * @bladestan-signature
 * @var \App\Models\AuthorCollection $users
 */
@endphp

@extends('welcome')

@section('content')
    <h1>{{ __('messages.welcome') }} - Admin</h1>

    <table>
        <thead>
            <tr>
                <th>Name</th>
                <th>Email</th>
                <th>Role</th>
            </tr>
        </thead>
        <tbody>
            @foreach($users->active()->byName() as $user)
                @php $rowLabel = 'Author: ' . $user->name; @endphp
                {{-- Bound component attributes: the expressions below are real
                     PHP, so $rowLabel (used only here) is not "unused" and
                     $user->email resolves for hover/go-to-definition. --}}
                <tr :data-label="$rowLabel" :data-email="$user->email">
                    <td>{{ $user->name }}</td>
                    <td>{{ $user->email }}</td>
                </tr>
            @endforeach
        </tbody>
    </table>

    @if($users->isEmpty())
        <p>{{ trans('pagination.next') }}</p>
    @endif
@endsection
