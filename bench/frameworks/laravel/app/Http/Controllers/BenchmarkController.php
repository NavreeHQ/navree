<?php

namespace App\Http\Controllers;

use App\Models\World;
use Illuminate\Http\JsonResponse;
use Illuminate\Http\Request;

/** The framework-overhead endpoints (SPEC.md §3.1 to §3.3). */
class BenchmarkController extends Controller
{
    public function json(): JsonResponse
    {
        return response()->json(['message' => 'Hello, World!']);
    }

    public function db(): JsonResponse
    {
        return response()->json($this->randomWorld());
    }

    public function queries(Request $request): JsonResponse
    {
        // Missing or non-numeric means 1; clamped to 1..500.
        $count = min(500, max(1, $request->integer('queries', 1)));

        $worlds = [];
        for ($i = 0; $i < $count; $i++) {
            $worlds[] = $this->randomWorld();
        }

        return response()->json($worlds);
    }

    /** One row, one query: SELECT * FROM world WHERE id = ? LIMIT 1. */
    private function randomWorld(): array
    {
        $world = World::find(random_int(1, 10000));

        return ['id' => $world->id, 'randomNumber' => $world->randomnumber];
    }
}
