<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;

/**
 * A shared base model: the columns and casts it declares configure every
 * pastry model that extends it.
 */
abstract class Pastry extends Model
{
    /** @var list<string> */
    protected $fillable = ['sku'];

    /** @var array<string, string> */
    protected $casts = ['is_vegan' => 'boolean'];
}
