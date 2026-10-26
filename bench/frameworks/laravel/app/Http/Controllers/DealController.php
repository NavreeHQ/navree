<?php

namespace App\Http\Controllers;

use App\Http\Requests\ListDealsRequest;
use App\Http\Requests\StoreDealRequest;
use App\Http\Resources\DealResource;
use App\Models\Deal;
use Illuminate\Http\Resources\Json\AnonymousResourceCollection;

class DealController extends Controller
{
    /** GET /api/deals: a count query, the page query and one eager-load query for the owners. */
    public function index(ListDealsRequest $request): AnonymousResourceCollection
    {
        $deals = Deal::with('owner')
            ->orderByDesc('created_at')
            ->orderByDesc('id')
            ->paginate(
                perPage: $request->integer('per_page', 20),
                page: $request->integer('page', 1),
            );

        return DealResource::collection($deals);
    }

    /** POST /api/deals: 201 with the new deal (the resource sets 201 for a just-created model). */
    public function store(StoreDealRequest $request): DealResource
    {
        $user = $request->user();

        $deal = $user->deals()->create($request->validated());

        return new DealResource($deal->setRelation('owner', $user));
    }
}
