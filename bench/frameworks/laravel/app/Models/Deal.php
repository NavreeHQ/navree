<?php

namespace App\Models;

use App\Enums\DealStage;
use Illuminate\Database\Eloquent\Attributes\Fillable;
use Illuminate\Database\Eloquent\Concerns\HasUuids;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Support\Carbon;

/**
 * @property string $id
 * @property string $title
 * @property int $amount_cents
 * @property DealStage $stage
 * @property string $owner_id
 * @property Carbon $created_at
 * @property Carbon $updated_at
 */
#[Fillable(['title', 'amount_cents', 'stage'])]
class Deal extends Model
{
    use HasUuids;

    /**
     * @return array<string, string>
     */
    protected function casts(): array
    {
        return [
            'amount_cents' => 'integer',
            'stage' => DealStage::class,
        ];
    }

    /**
     * @return BelongsTo<User, $this>
     */
    public function owner(): BelongsTo
    {
        return $this->belongsTo(User::class, 'owner_id');
    }
}
