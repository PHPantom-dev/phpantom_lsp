<?php

namespace App\Http\Controllers;

use App\Models\Bakery;
use Illuminate\Http\JsonResponse;
use Illuminate\View\View;

class BakeryController
{
    public function index(): View
    {
        return view('welcome', [
            'bakeries' => Bakery::where('open', true)->freshlyBaked()->get(),
        ]);
    }

    public function show(Bakery $bakery): JsonResponse
    {
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
