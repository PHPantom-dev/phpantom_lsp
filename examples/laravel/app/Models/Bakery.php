<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Attributes\Scope;
use Illuminate\Database\Eloquent\Builder;
use Illuminate\Database\Eloquent\Casts\Attribute;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsToMany;
use Illuminate\Database\Eloquent\Relations\HasMany;
use Illuminate\Database\Eloquent\Relations\HasOne;

class Bakery extends Model
{
    protected $fillable = ['flour'];

    protected $guarded = ['kitchen_id'];

    protected $hidden = ['oven_code'];

    protected $dates = ['defrosted_at'];

    protected $visible = ['rye_blend'];

    protected $appends = ['warmth'];

    protected $casts = [
        'apricot'    => 'boolean',
        'dough_temp' => 'float',
        'icing'      => FrostingCast::class,
        'jam_flavor' => JamFlavor::class,
        'notes'      => 'array',
        'proved_at'  => 'datetime',
    ];

    protected function casts(): array
    {
        return [
            'quality' => 'float',
        ];
    }

    protected $attributes = [
        'croissant'   => 'plain',
        'egg_count'   => 0,
        'gluten_free' => false,
    ];

    /** @return HasMany<Loaf, $this> */
    public function baguettes(): mixed { return $this->hasMany(Loaf::class); }

    /** @return HasOne<Baker, $this> */
    public function headBaker(): mixed { return $this->hasOne(Baker::class); }

    /** @return BelongsToMany<BakeryRecipe, $this> */
    public function masterRecipe(): mixed { return $this->belongsToMany(BakeryRecipe::class)->using(RecipeIngredient::class)->withPivot('quantity', 'unit'); }

    public function vendor() { return $this->morphTo(); }

    public function scopeTopping(Builder $query, string $type): void
    {
        $query->where('topping', $type);
    }

    public function scopeUnbaked(Builder $query): void
    {
        $query->where('baked', false);
    }

    #[Scope]
    protected function freshlyBaked(Builder $query): void
    {
        $query->where('fresh', true);
    }

    public function getLoafNameAttribute(): string { return ''; }

    /** @return Attribute<string> */
    protected function sprinkle(): Attribute
    {
        return new Attribute();
    }
}
