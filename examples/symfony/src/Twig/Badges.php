<?php

namespace App\Twig;

use Twig\Attribute\AsTwigFunction;

final class Badges
{
    /**
     * A badge reading the given label.
     */
    #[AsTwigFunction('badge', isSafe: ['html'])]
    public static function badge(string $label): string
    {
        return '<span class="badge">' . htmlspecialchars($label) . '</span>';
    }
}
