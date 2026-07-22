<?php

namespace Database\Factories;

use Illuminate\Database\Eloquent\Factories\Factory;

/**
 * Convention-based factory.
 *
 * There is no `@extends Factory<Model>` generic here on purpose: PHPantom
 * derives the model (App\Models\BlogAuthor) from the factory class name and
 * synthesizes create()/make() returning the model, plus the dynamic
 * has{Relationship}() / for{Relationship}() methods (hasPosts(), hasProfile(),
 * forPosts(), forProfile()) for each relationship on the model — each
 * returning the factory so the chain continues.
 */
class BlogAuthorFactory extends Factory
{
    public function definition(): array
    {
        return [
            'name' => 'Ada Lovelace',
            'email' => 'ada@example.com',
            'genre' => 'science',
            'active' => true,
        ];
    }
}
