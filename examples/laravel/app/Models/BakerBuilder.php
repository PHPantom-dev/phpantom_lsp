<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Builder;

/**
 * @template TModel of \App\Models\Baker
 * @extends Builder<TModel>
 * @mixin Baker
 */
class BakerBuilder extends Builder
{
    /**
     * @return $this
     */
    public function active()
    {
        return $this->where('active', true);
    }
}
