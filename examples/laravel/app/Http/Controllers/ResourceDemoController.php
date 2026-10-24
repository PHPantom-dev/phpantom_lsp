<?php

namespace App\Http\Controllers;

use Illuminate\Http\JsonResponse;
use Illuminate\Http\Request;

/** Stateless responses for the resource-registration demos. */
class ResourceDemoController
{
    public function index(): JsonResponse
    {
        return response()->json(['action' => 'index']);
    }

    public function create(): JsonResponse
    {
        return response()->json(['action' => 'create', 'fields' => ['name']]);
    }

    public function store(Request $request): JsonResponse
    {
        return response()->json(['action' => 'store', 'submitted' => $request->only(['name'])]);
    }

    public function show(Request $request): JsonResponse
    {
        return response()->json(['action' => 'show', 'resource' => $request->path()]);
    }

    public function edit(Request $request): JsonResponse
    {
        return response()->json(['action' => 'edit', 'resource' => $request->path(), 'fields' => ['name']]);
    }

    public function update(Request $request): JsonResponse
    {
        return response()->json(['action' => 'update', 'submitted' => $request->only(['name'])]);
    }

    public function destroy(Request $request): JsonResponse
    {
        return response()->json(['action' => 'destroy', 'resource' => $request->path()]);
    }
}
