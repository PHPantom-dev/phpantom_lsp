<?php

declare(strict_types=1);

namespace Drupal\acme;

use Drupal\Core\Config\ConfigFactoryInterface;
use Drupal\Core\Security\TrustedCallbackInterface;
use Drupal\Core\StringTranslation\StringTranslationTrait;

/**
 * Builds the greeting the demo route shows.
 */
final class Greeter implements TrustedCallbackInterface
{
    use StringTranslationTrait;

    public function __construct(
        private readonly ConfigFactoryInterface $configFactory,
        private readonly string $format,
    ) {}

    public function greet(string $name): string
    {
        // Try: complete after `$this->configFactory->`. The methods come
        // from core's ConfigFactoryInterface. Go to the definition of
        // 'system.site', which lists core's default configuration and schema
        // entry, or complete a configuration name inside `get('')`. Complete
        // a key of system.site inside the second `get('')`.
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

    /**
     * The greeting as a render array, for the card's lazy builder.
     *
     * @return array<string, mixed>
     */
    public function buildGreeting(string $name): array
    {
        return ['#markup' => $this->greet($name)];
    }

    public static function trustedCallbacks(): array
    {
        return ['buildGreeting'];
    }
}
