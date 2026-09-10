<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Relations\Pivot;

/**
 * Custom pivot model for the Bakery ↔ BakeryRecipe many-to-many relationships,
 * wired via `->using(RecipeIngredient::class)` on `Bakery::masterRecipe()`,
 * where it is reached as `$pivot`, and on `Bakery::seasonalRecipes()`, which
 * renames the accessor to `$ingredient` with `->as('ingredient')`.
 */
class RecipeIngredient extends Pivot
{
    public function getQuantityLabel(): string { return ''; }
}
