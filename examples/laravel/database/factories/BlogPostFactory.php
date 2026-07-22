<?php

namespace Database\Factories;

use Illuminate\Database\Eloquent\Factories\Factory;

/**
 * Convention-based factory for App\Models\BlogPost.
 *
 * BlogPost has a `belongsTo` author relationship, so PHPantom synthesizes
 * forAuthor()/hasAuthor().  Because BlogPost uses the SoftDeletes trait, the
 * factory also gains a trashed() method.  All three return the factory so
 * they chain into create()/make().
 */
class BlogPostFactory extends Factory
{
    public function definition(): array
    {
        return [
            'title' => 'Notes on the Analytical Engine',
            'slug' => 'notes-on-the-analytical-engine',
            'published' => true,
        ];
    }
}
