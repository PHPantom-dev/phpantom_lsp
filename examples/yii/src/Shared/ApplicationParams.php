<?php

declare(strict_types=1);

namespace App\Shared;

final readonly class ApplicationParams
{
    public function __construct(
        public string $name = 'My Project',
        public string $charset = 'UTF-8',
        public string $locale = 'en',
    ) {}
}
