<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Attributes\UseEloquentBuilder;

#[UseEloquentBuilder(BakerBuilder::class)]
class Baker extends Model
{
    public function getName(): string { return ''; }
}
