<?php

declare(strict_types=1);

namespace Drupal\acme\Controller;

use Drupal\acme\Greeter;
use Drupal\Core\Controller\ControllerBase;

/**
 * Shows the greeting for the acme.greeting route.
 */
final class GreetingController extends ControllerBase
{
    // ControllerBase autowires this argument through the Greeter alias in
    // acme.services.yml.
    public function __construct(
        private readonly Greeter $greeter,
    ) {}

    /**
     * @return array<string, mixed>
     */
    public function greet(string $name): array
    {
        // Try: go to the definition of `$this->greeter->greet()`, or complete
        // after `$this->` to see the helpers ControllerBase inherits from core.
        return [
            '#theme' => 'acme_greeting',
            '#name' => $name,
            '#greeting' => $this->greeter->greet($name),
        ];
    }
}
