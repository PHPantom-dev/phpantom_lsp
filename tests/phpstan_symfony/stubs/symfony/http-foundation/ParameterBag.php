<?php

/*
 * This file is part of the Symfony package.
 *
 * (c) Fabien Potencier <fabien@symfony.com>
 *
 * For the full copyright and license information, please view the LICENSE
 * file that was distributed with this source code.
 */

namespace Symfony\Component\HttpFoundation;

use Symfony\Component\HttpFoundation\Exception\BadRequestException;
use Symfony\Component\HttpFoundation\Exception\UnexpectedValueException;

/**
 * ParameterBag is a container for key/value pairs.
 *
 * @author Fabien Potencier <fabien@symfony.com>
 *
 * @implements \IteratorAggregate<string, mixed>
 */
class ParameterBag implements \IteratorAggregate, \Countable
{
    /**
     * @param array<string, mixed> $parameters
     */
    public function __construct(
        protected array $parameters = [],
    ) {}

    /**
     * Returns the parameters.
     *
     * @template TKey of string|null
     *
     * @param TKey $key The name of the parameter to return or null to get them all
     *
     * @return (TKey is null ? array<string, mixed> : array<mixed>)
     *
     * @throws BadRequestException if the value is not an array
     */
    public function all(?string $key = null): array
    {}

    /**
     * Returns the parameter keys.
     *
     * @return list<string>
     */
    public function keys(): array
    {}

    /**
     * Replaces the current parameters by a new set.
     *
     * @param array<string, mixed> $parameters
     */
    public function replace(array $parameters = []): void
    {}

    /**
     * Adds parameters.
     *
     * @param array<string, mixed> $parameters
     */
    public function add(array $parameters = []): void
    {}

    public function get(string $key, mixed $default = null): mixed
    {}

    public function set(string $key, mixed $value): void
    {}

    /**
     * Returns true if the parameter is defined.
     */
    public function has(string $key): bool
    {}

    /**
     * Removes a parameter.
     */
    public function remove(string $key): void
    {}

    /**
     * Returns the alphabetic characters of the parameter value.
     *
     * @throws UnexpectedValueException if the value cannot be converted to string
     */
    public function getAlpha(string $key, string $default = ''): string
    {}

    /**
     * Returns the alphabetic characters and digits of the parameter value.
     *
     * @throws UnexpectedValueException if the value cannot be converted to string
     */
    public function getAlnum(string $key, string $default = ''): string
    {}

    /**
     * Returns the digits of the parameter value.
     *
     * @throws UnexpectedValueException if the value cannot be converted to string
     */
    public function getDigits(string $key, string $default = ''): string
    {}

    /**
     * Returns the parameter as string.
     *
     * @throws UnexpectedValueException if the value cannot be converted to string
     */
    public function getString(string $key, string $default = ''): string
    {}

    /**
     * Returns the parameter value converted to integer.
     *
     * @throws UnexpectedValueException if the value cannot be converted to integer
     */
    public function getInt(string $key, int $default = 0): int
    {}

    /**
     * Returns the parameter value converted to boolean.
     *
     * @throws UnexpectedValueException if the value cannot be converted to a boolean
     */
    public function getBoolean(string $key, bool $default = false): bool
    {}

    /**
     * Returns the parameter value converted to an enum.
     *
     * @template T of \BackedEnum
     *
     * @param class-string<T> $class
     * @param ?T              $default
     *
     * @return ?T
     *
     * @psalm-return ($default is null ? T|null : T)
     *
     * @throws UnexpectedValueException if the parameter value cannot be converted to an enum
     */
    public function getEnum(string $key, string $class, ?\BackedEnum $default = null): ?\BackedEnum
    {}

    /**
     * Filter key.
     *
     * @param int                                     $filter  FILTER_* constant
     * @param int|array{flags?: int, options?: array} $options Flags from FILTER_* constants
     *
     * @see https://php.net/filter-var
     *
     * @throws UnexpectedValueException if the parameter value is a non-stringable object
     * @throws UnexpectedValueException if the parameter value is invalid and \FILTER_NULL_ON_FAILURE is not set
     */
    public function filter(string $key, mixed $default = null, int $filter = \FILTER_DEFAULT, mixed $options = []): mixed
    {}

    /**
     * Returns an iterator for parameters.
     *
     * @return \ArrayIterator<string, mixed>
     */
    public function getIterator(): \ArrayIterator
    {}

    /**
     * Returns the number of parameters.
     */
    public function count(): int
    {}
}
