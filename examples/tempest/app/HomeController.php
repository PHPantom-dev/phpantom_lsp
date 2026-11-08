<?php

declare(strict_types=1);

namespace App;

use Tempest\Router\Get;

final class HomeController
{
    #[Get('/home/{locale}')]
    public function __invoke(string $locale): string
    {
        return $locale;
    }
}
