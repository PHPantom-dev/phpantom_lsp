<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Attributes\CollectedBy;
use Illuminate\Database\Eloquent\Builder;
use Illuminate\Database\Eloquent\Factories\HasFactory;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\HasMany;
use Illuminate\Database\Eloquent\Relations\HasOne;

/**
 * @property-read string $displayName
 * @method string bio()
 * @method static Builder<static> withTrashed(bool $withTrashed = true)
 * @method static Builder<static> onlyTrashed()
 */
#[CollectedBy(AuthorCollection::class)]
class BlogAuthor extends Model
{
    // Convention-based factory: PHPantom maps BlogAuthor to
    // Database\Factories\BlogAuthorFactory (no @use generic needed).
    use HasFactory;

    protected $fillable = ['name', 'email', 'genre'];

    protected $casts = [
        'active' => 'bool',
    ];

    protected $guarded = ['id'];

    protected $hidden = ['password'];

    /** @return HasMany<BlogPost, $this> */
    public function posts(): mixed { return $this->hasMany(BlogPost::class); }

    /** @return HasOne<AuthorProfile, $this> */
    public function profile(): mixed { return $this->hasOne(AuthorProfile::class); }

    public function scopeActive(Builder $query): void
    {
        $query->where('active', true);
    }

    public function scopeOfGenre(Builder $query, string $genre): void
    {
        $query->where('genre', $genre);
    }
}
