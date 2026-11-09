<?php

declare(strict_types=1);

namespace Drupal\acme\Element;

use Drupal\Core\Render\Attribute\RenderElement;
use Drupal\Core\Render\Element\RenderElementBase;

/**
 * A card that frames a greeting, declared by attribute the way Drupal 11
 * core declares its own render elements.
 */
#[RenderElement('acme_greeting_card')]
final class GreetingCard extends RenderElementBase
{
    /**
     * @return array<string, mixed>
     */
    public function getInfo(): array
    {
        return [
            '#theme_wrappers' => ['container'],
            '#attributes' => ['class' => ['acme-greeting-card']],
        ];
    }
}
