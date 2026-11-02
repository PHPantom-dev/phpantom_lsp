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

    /**
     * Implements hook_acme_greeting_alter().
     */
    #[Hook('acme_greeting_alter')]
    public function greetingAlter(string &$greeting, $name): void
    {
        // Try: hover `$name`, typed `string` by the hook's documentation in
        // acme.api.php, or go to the definition of 'acme_greeting_alter',
        // which lists the alter() call in Greeter and the documentation.
        if ($name === 'Ada') {
            $greeting .= ' Welcome back.';
        }
    }
}
