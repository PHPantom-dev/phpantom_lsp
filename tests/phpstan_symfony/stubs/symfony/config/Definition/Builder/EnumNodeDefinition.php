<?php

/*
 * This file is part of the Symfony package.
 *
 * (c) Fabien Potencier <fabien@symfony.com>
 *
 * For the full copyright and license information, please view the LICENSE
 * file that was distributed with this source code.
 */

namespace Symfony\Component\Config\Definition\Builder;

use Symfony\Component\Config\Definition\EnumNode;

/**
 * Enum Node Definition.
 *
 * @template TParent of NodeParentInterface|null
 *
 * @extends ScalarNodeDefinition<TParent>
 *
 * @author Johannes M. Schmitt <schmittjoh@gmail.com>
 */
class EnumNodeDefinition extends ScalarNodeDefinition
{
    private array $values;
    private string $enumFqcn;

    /**
     * @return $this
     */
    public function values(array $values): static
    {}

    /**
     * @param class-string<\UnitEnum> $enumFqcn
     *
     * @return $this
     */
    public function enumFqcn(string $enumFqcn): static
    {}

    /**
     * @throws \RuntimeException when no values or enumFqcn is set
     */
    protected function instantiateNode(): EnumNode
    {}
}
