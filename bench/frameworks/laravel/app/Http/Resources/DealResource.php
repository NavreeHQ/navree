<?php

namespace App\Http\Resources;

use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

/** @mixin \App\Models\Deal */
class DealResource extends JsonResource
{
    /**
     * @return array<string, mixed>
     */
    public function toArray(Request $request): array
    {
        return [
            'id' => $this->id,
            'title' => $this->title,
            'amount_cents' => $this->amount_cents,
            'stage' => $this->stage,
            'owner' => new OwnerResource($this->whenLoaded('owner')),
            'created_at' => $this->created_at,
        ];
    }
}
