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

        $greeting = strtr($this->format, [
            '@name' => $name,
            '@site' => is_string($siteName) ? $siteName : 'Drupal',
        ]);
        // Try: go to the definition of 'acme_greeting', which lists the
        // implementation in AcmeHooks and then the documentation in
        // acme.api.php. Complete a hook inside `alter('')`.
        \Drupal::moduleHandler()->alter('acme_greeting', $greeting, $name);

        return $greeting;
    }
}
