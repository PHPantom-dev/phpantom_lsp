<?php

/*
 * This file is part of the Symfony package.
 *
 * (c) Fabien Potencier <fabien@symfony.com>
 *
 * For the full copyright and license information, please view the LICENSE
 * file that was distributed with this source code.
 */

namespace Symfony\Component\DependencyInjection\ParameterBag;

use Symfony\Component\DependencyInjection\Exception\EmptyParameterValueException;
use Symfony\Component\DependencyInjection\Exception\InvalidArgumentException;
use Symfony\Component\DependencyInjection\Exception\ParameterCircularReferenceException;
use Symfony\Component\DependencyInjection\Exception\ParameterNotFoundException;
use Symfony\Component\DependencyInjection\Exception\RuntimeException;

/**
 * Holds parameters.
 *
 * @author Fabien Potencier <fabien@symfony.com>
 */
class ParameterBag implements ParameterBagInterface
{
    protected array $parameters = [];
    protected bool $resolved = false;
    protected array $deprecatedParameters = [];
    protected array $nonEmptyParameters = [];

    public function __construct(array $parameters = [])
    {}

    public function clear(): void
    {}

    public function add(array $parameters): void
    {}

    public function all(): array
    {}

    public function allDeprecated(): array
    {}

    public function allNonEmpty(): array
    {}

    public function get(string $name): array|bool|string|int|float|\UnitEnum|null
    {}

    public function set(string $name, array|bool|string|int|float|\UnitEnum|null $value): void
    {}

    /**
     * Deprecates a service container parameter.
     *
     * @throws ParameterNotFoundException if the parameter is not defined
     */
    public function deprecate(string $name, string $package, string $version, string $message = 'The parameter "%s" is deprecated.'): void
    {}

    public function cannotBeEmpty(string $name, string $message): void
    {}

    public function has(string $name): bool
    {}

    public function remove(string $name): void
    {}

    public function resolve(): void
    {}

    /**
     * Replaces parameter placeholders (%name%) by their values.
     *
     * @template TValue of array<array|scalar>|scalar
     *
     * @param TValue $value
     * @param array  $resolving An array of keys that are being resolved (used internally to detect circular references)
     *
     * @psalm-return (TValue is scalar ? array|scalar : array<array|scalar>)
     *
     * @throws ParameterNotFoundException          if a placeholder references a parameter that does not exist
     * @throws ParameterCircularReferenceException if a circular reference if detected
     * @throws RuntimeException                    when a given parameter has a type problem
     */
    public function resolveValue(mixed $value, array $resolving = []): mixed
    {}

    /**
     * Resolves parameters inside a string.
     *
     * @param array $resolving An array of keys that are being resolved (used internally to detect circular references)
     *
     * @throws ParameterNotFoundException          if a placeholder references a parameter that does not exist
     * @throws ParameterCircularReferenceException if a circular reference if detected
     * @throws RuntimeException                    when a given parameter has a type problem
     */
    public function resolveString(string $value, array $resolving = []): mixed
    {}

    public function isResolved(): bool
    {}

    public function escapeValue(mixed $value): mixed
    {}

    public function unescapeValue(mixed $value): mixed
    {}
}
