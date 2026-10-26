<?php

namespace App\Http\Requests;

use App\Enums\DealStage;
use Illuminate\Foundation\Http\FormRequest;
use Illuminate\Validation\Rule;

class StoreDealRequest extends FormRequest
{
    /**
     * @return array<string, array<int, mixed>>
     */
    public function rules(): array
    {
        return [
            'title' => ['required', 'string', 'max:200'],
            // "strict": a JSON integer, not a numeric string or a boolean.
            'amount_cents' => ['required', 'integer:strict', 'min:0', 'max:1000000000000'],
            'stage' => ['required', Rule::enum(DealStage::class)],
        ];
    }
}
