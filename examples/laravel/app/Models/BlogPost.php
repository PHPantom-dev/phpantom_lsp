<?php

namespace App\Models;

use App\Observers\BlogPostObserver;
use Illuminate\Database\Eloquent\Attributes\CollectedBy;
use Illuminate\Database\Eloquent\Attributes\ObservedBy;
use Illuminate\Database\Eloquent\Factories\HasFactory;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Database\Eloquent\SoftDeletes;

/**
 * Try: the code lenses above the class go to the classes Laravel pairs with
 * it: its factory (BlogPostFactory, by `HasFactory`'s naming convention), its
 * policy (PublishingPolicy, registered in DemoServiceProvider), its observer
 * (BlogPostObserver, from `#[ObservedBy]`), and its collection (PostCollection).
 */
#[CollectedBy(PostCollection::class)]
#[ObservedBy(BlogPostObserver::class)]
class BlogPost extends Model
{
    // SoftDeletes enables the factory's synthesized trashed() method.
    use HasFactory;
    use SoftDeletes;

    protected $fillable = ['title', 'slug'];

    protected $casts = [
        'published' => 'bool',
    ];

    public function getTitle(): string { return $this->title; }
    public function getSlug(): string { return $this->slug; }

    /** @return BelongsTo<BlogAuthor, covariant $this> */
    public function author(): mixed { return $this->belongsTo(BlogAuthor::class); }
}
