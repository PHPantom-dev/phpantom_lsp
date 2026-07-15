<?php

namespace Demo\Common;

use Illuminate\Support\Collection;
use Illuminate\Support\ServiceProvider;

class CommonServiceProvider extends ServiceProvider
{
    public function boot(): void
    {
        Collection::macro('toUpper', function (): Collection {
            return $this->map(fn ($value) => strtoupper($value));
        });
    }
}
