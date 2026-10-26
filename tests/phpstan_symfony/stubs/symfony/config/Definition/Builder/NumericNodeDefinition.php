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

use Symfony\Component\Config\Definition\Exception\InvalidDefinitionException;

/**
 * Abstract class that contains common code of integer and float node definitions.
 *
 * @template TParent of NodeParentInterface|null = null
 *
 * @extends ScalarNodeDefinition<TParent>
 *
 * @author David Jeanmonod <david.jeanmonod@gmail.com>
 */
abstract class NumericNodeDefinition extends ScalarNodeDefinition
{
    protected int|float|null $min = null;
    protected int|float|null $max = null;

    /**
     * Ensures that the value is smaller than the given reference.
     *
     * @return $this
     *
     * @throws \InvalidArgumentException when the constraint is inconsistent
     */
    public function max(int|float $max): static
    {}

    /**
     * Ensures that the value is bigger than the given reference.
     *
     * @return $this
     *
     * @throws \InvalidArgumentException when the constraint is inconsistent
     */
    public function min(int|float $min): static
    {}

    /**
     * @throws InvalidDefinitionException
     */
    public function cannotBeEmpty(): static
    {}
}
