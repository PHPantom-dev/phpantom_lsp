<?php

namespace App\Http\Controllers;

use App\Models\Bakery;
use Illuminate\Foundation\Auth\Access\AuthorizesRequests;
use Illuminate\Http\JsonResponse;
use Illuminate\View\View;

class BakeryController
{
    use AuthorizesRequests;

    public function index(): View
    {
        // `$this->authorize()` names an ability of the subject's policy, the
        // same set `Gate::allows()` checks against.
        $this->authorize('viewAny', Bakery::class);

        return view('welcome', [
            'bakeries' => Bakery::where('open', true)->freshlyBaked()->get(),
        ]);
    }

    public function show(Bakery $bakery): JsonResponse
    {
        $this->authorize('update', $bakery);

        return response()->json([
            'id' => $bakery->id,
            'name' => $bakery->loaf_name,
        ]);
    }

    public function cancel(Bakery $bakery): JsonResponse
    {
        return response()->json([
            'cancelled' => $bakery->id,
        ]);
    }
}
