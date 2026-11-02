<?php

namespace App\Twig;

use Twig\Extension\AbstractExtension;
use Twig\TwigFilter;

final class AppExtension extends AbstractExtension
{
    public function getFilters(): array
    {
        return [
            // Try: find references on 'initials'. templates/demo/index.html.twig
            // applies it.
            new TwigFilter('initials', [$this, 'initials']),
        ];
    }

    /**
     * The first letter of each word in a name.
     */
    public function initials(string $name): string
    {
        $letters = array_map(
            static fn (string $word): string => mb_substr($word, 0, 1),
            preg_split('/\s+/', trim($name)) ?: [],
        );

        return mb_strtoupper(implode('', $letters));
    }
}
