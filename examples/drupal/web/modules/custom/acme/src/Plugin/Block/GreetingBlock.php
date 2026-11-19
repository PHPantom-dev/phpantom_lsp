<?php

declare(strict_types=1);

namespace Drupal\acme\Plugin\Block;

use Drupal\Core\Block\Attribute\Block;
use Drupal\Core\Block\BlockBase;
use Drupal\Core\StringTranslation\TranslatableMarkup;

/**
 * A block that greets the world, declared by attribute the way Drupal 11
 * core declares its own blocks.
 */
#[Block(
    id: 'acme_greeting_block',
    admin_label: new TranslatableMarkup('Greeting'),
)]
final class GreetingBlock extends BlockBase
{
    /**
     * @return array<string, mixed>
     */
    public function build(): array
    {
        return ['#markup' => $this->greeting()];
    }

    public function greeting(): string
    {
        return 'Hello, World!';
    }
}
