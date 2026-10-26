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

use Symfony\Component\Config\Definition\Exception\UnsetKeyException;

/**
 * This class builds an if expression.
 *
 * @template T of NodeDefinition
 *
 * @author Johannes M. Schmitt <schmittjoh@gmail.com>
 * @author Christophe Coevoet <stof@notk.org>
 */
class ExprBuilder
{
    public const TYPE_ANY = 'any';
    public const TYPE_STRING = 'string';
    public const TYPE_NULL = 'null';
    public const TYPE_ARRAY = 'array';
    public const TYPE_BOOL = 'bool';
    public const TYPE_INT = 'int';
    public const TYPE_BACKED_ENUM = 'backed-enum';

    public string $allowedTypes;
    public ?\Closure $ifPart = null;
    public ?\Closure $thenPart = null;

    /**
     * @param T $node
     */
    public function __construct(
        protected NodeDefinition $node,
    ) {}

    /**
     * Marks the expression as being always used.
     *
     * @return $this
     */
    public function always(?\Closure $then = null): static
    {}

    /**
     * Sets a closure to use as tests.
     *
     * The default one tests if the value is true.
     *
     * @return $this
     */
    public function ifTrue(?\Closure $closure = null): static
    {}

    /**
     * Sets a closure to use as tests.
     *
     * The default one tests if the value is false.
     *
     * @return $this
     */
    public function ifFalse(?\Closure $closure = null): static
    {}

    /**
     * Tests if the value is a string.
     *
     * @return $this
     */
    public function ifString(): static
    {}

    /**
     * Tests if the value is null.
     *
     * @return $this
     */
    public function ifNull(): static
    {}

    /**
     * Tests if the value is empty.
     *
     * @return $this
     */
    public function ifEmpty(): static
    {}

    /**
     * Tests if the value is an array.
     *
     * @return $this
     */
    public function ifArray(): static
    {}

    /**
     * Tests if the value is in an array.
     *
     * @return $this
     */
    public function ifInArray(array $array): static
    {}

    /**
     * Tests if the value is not in an array.
     *
     * @return $this
     */
    public function ifNotInArray(array $array): static
    {}

    /**
     * Transforms variables of any type into an array.
     *
     * @return $this
     */
    public function castToArray(): static
    {}

    /**
     * Sets the closure to run if the test pass.
     *
     * @return $this
     */
    public function then(\Closure $closure): static
    {}

    /**
     * Sets a closure returning an empty array.
     *
     * @return $this
     */
    public function thenEmptyArray(): static
    {}

    /**
     * Sets a closure marking the value as invalid at processing time.
     *
     * if you want to add the value of the node in your message just use a %s placeholder.
     *
     * @return $this
     *
     * @throws \InvalidArgumentException
     */
    public function thenInvalid(string $message): static
    {}

    /**
     * Sets a closure unsetting this key of the array at processing time.
     *
     * @return $this
     *
     * @throws UnsetKeyException
     */
    public function thenUnset(): static
    {}

    /**
     * Returns the related node.
     *
     * @return T
     *
     * @throws \RuntimeException
     */
    public function end(): NodeDefinition
    {}

    /**
     * Builds the expressions.
     *
     * @param (ExprBuilder|\Closure)[] $expressions
     *
     * @return \Closure[]
     */
    public static function buildExpressions(array $expressions): array
    {}
}
