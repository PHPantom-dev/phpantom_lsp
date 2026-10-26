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

use Symfony\Component\Config\Definition\BooleanNode;
use Symfony\Component\Config\Definition\Exception\InvalidDefinitionException;

/**
 * This class provides a fluent interface for defining a node.
 *
 * @template TParent of NodeParentInterface|null
 *
 * @extends ScalarNodeDefinition<TParent>
 *
 * @author Johannes M. Schmitt <schmittjoh@gmail.com>
 */
class BooleanNodeDefinition extends ScalarNodeDefinition
{
    /**
     * @param TParent $parent
     */
    public function __construct(?string $name, ?NodeParentInterface $parent = null)
    {}

    protected function instantiateNode(): BooleanNode
    {}

    /**
     * @throws InvalidDefinitionException
     */
    public function cannotBeEmpty(): static
    {}

    /**
     * @return $this
     */
    public function defaultValue(mixed $value): static
    {}
}
