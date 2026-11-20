<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Builder;
use Illuminate\Database\Eloquent\Attributes\Scope;
use Illuminate\Database\Eloquent\Attributes\UseEloquentBuilder;

/** @method static BakerBuilder<static>|Baker employed() */
#[UseEloquentBuilder(BakerBuilder::class)]
class Baker extends Model
{
    public function getName(): string { return ''; }

    protected function scopeEmployed(Builder $query): Builder
    {
        return $query->whereNotNull('hired_at');
    }

    #[Scope]
    protected function available(Builder $query): void
    {
        $query->where('available', true);
    }
}
