<?php

declare(strict_types=1);

namespace Drupal\acme\Plugin\Block;

use Drupal\Core\Block\BlockBase;

/**
 * A block declared by annotation, the way code written before Drupal 10.2
 * declares its plugins.
 *
 * Try: go to the definition of `Block` or `Translation` below. Neither is
 * imported, so both are found the way plugin discovery finds them: in the
 * namespace of the block annotation class and in core's annotations.
 *
 * @Block(
 *   id = "acme_farewell_block",
 *   admin_label = @Translation("Farewell"),
 * )
 */
final class FarewellBlock extends BlockBase
{
    /**
     * @return array<string, mixed>
     */
    public function build(): array
    {
        return ['#markup' => 'Goodbye, World!'];
    }
}
