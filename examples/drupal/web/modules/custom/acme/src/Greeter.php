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
        private readonly string $format,
    ) {}

    public function greet(string $name): string
    {
        // Try: complete after `$this->configFactory->`. The methods come
        // from core's ConfigFactoryInterface.
        $siteName = $this->configFactory->get('system.site')->get('name');

        return strtr($this->format, [
            '@name' => $name,
            '@site' => is_string($siteName) ? $siteName : 'Drupal',
        ]);
    }
}
