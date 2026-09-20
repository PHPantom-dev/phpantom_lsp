<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\HasOne;

/**
 * @template TParent of Model
 * @template TRelated of Model
 * @extends HasOne<TRelated, TParent>
 */
class BakerRelation extends HasOne
{
}
