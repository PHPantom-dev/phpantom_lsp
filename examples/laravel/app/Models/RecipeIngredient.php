<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Relations\Pivot;

/**
 * Custom pivot model for the Bakery ↔ BakeryRecipe many-to-many relationship,
 * wired via `->using(RecipeIngredient::class)` on `Bakery::masterRecipe()`.
 */
class RecipeIngredient extends Pivot
{
    public function getQuantityLabel(): string { return ''; }
}
