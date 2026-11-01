<?php

declare(strict_types=1);

namespace Drupal\acme\Hook;

use Drupal\Core\Hook\Attribute\Hook;

/**
 * Hook implementations declared by attribute.
 */
final class AcmeHooks
{
    /**
     * Implements hook_theme().
     *
     * @return array<string, array<string, mixed>>
     */
    #[Hook('theme')]
    public function theme(): array
    {
        return [
            'acme_greeting' => [
                'variables' => [
                    'name' => null,
                    'greeting' => null,
                ],
            ],
        ];
    }
}
