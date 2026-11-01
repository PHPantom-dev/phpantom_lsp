<?php

declare(strict_types=1);

namespace Drupal\acme;

use Drupal\Core\Config\ConfigFactoryInterface;

/**
 * Builds the greeting the demo route shows.
 */
final class Greeter
{
    public function __construct(
        private readonly ConfigFactoryInterface $configFactory,
    ) {}

    public function greet(string $name): string
    {
        // Try: complete after `$this->configFactory->`. The methods come
        // from core's ConfigFactoryInterface.
        $siteName = $this->configFactory->get('system.site')->get('name');

        return sprintf('Hello %s, welcome to %s.', $name, is_string($siteName) ? $siteName : 'Drupal');
    }
}
