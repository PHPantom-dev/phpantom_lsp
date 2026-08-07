<?php

namespace App\Models;

use App\Policies\ReviewModerationPolicy;
use Illuminate\Database\Eloquent\Attributes\CollectedBy;
use Illuminate\Database\Eloquent\Attributes\UsePolicy;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\HasMany;
use Illuminate\Database\Eloquent\Relations\MorphTo;

#[CollectedBy(ReviewCollection::class)]
#[UsePolicy(ReviewModerationPolicy::class)]
class Review extends Model
{
    public function getTitle(): string { return ''; }
    public function getRating(): int { return 0; }

    /** @return HasMany<Review, $this> */
    public function replies(): mixed { return $this->hasMany(Review::class); }

    /**
     * A review belongs to whatever it reviews.  The `reviewable_type` column
     * holds the morph alias DemoServiceProvider registers for each model.
     *
     * @return MorphTo<Model, $this>
     */
    public function reviewable(): mixed { return $this->morphTo(); }
}
