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

/**
 * HeaderBag is a container for HTTP headers.
 *
 * @author Fabien Potencier <fabien@symfony.com>
 *
 * @implements \IteratorAggregate<string, list<string|null>>
 */
class HeaderBag implements \IteratorAggregate, \Countable, \Stringable
{
    protected const UPPER = '_ABCDEFGHIJKLMNOPQRSTUVWXYZ';
    protected const LOWER = '-abcdefghijklmnopqrstuvwxyz';

    /**
     * @var array<string, list<string|null>>
     */
    protected array $headers = [];
    protected array $cacheControl = [];

    public function __construct(array $headers = [])
    {}

    /**
     * Returns the headers as a string.
     */
    public function __toString(): string
    {}

    /**
     * Returns the headers.
     *
     * @param string|null $key The name of the headers to return or null to get them all
     *
     * @return ($key is null ? array<string, list<string|null>> : list<string|null>)
     */
    public function all(?string $key = null): array
    {}

    /**
     * Returns the parameter keys.
     *
     * @return string[]
     */
    public function keys(): array
    {}

    /**
     * Replaces the current HTTP headers by a new set.
     */
    public function replace(array $headers = []): void
    {}

    /**
     * Adds new headers the current HTTP headers set.
     */
    public function add(array $headers): void
    {}

    /**
     * Returns the first header by name or the default one.
     */
    public function get(string $key, ?string $default = null): ?string
    {}

    /**
     * Sets a header by name.
     *
     * @param string|string[]|null $values  The value or an array of values
     * @param bool                 $replace Whether to replace the actual value or not (true by default)
     */
    public function set(string $key, string|array|null $values, bool $replace = true): void
    {}

    /**
     * Returns true if the HTTP header is defined.
     */
    public function has(string $key): bool
    {}

    /**
     * Returns true if the given HTTP header contains the given value.
     */
    public function contains(string $key, string $value): bool
    {}

    /**
     * Removes a header.
     */
    public function remove(string $key): void
    {}

    /**
     * Returns the HTTP header value converted to a date.
     *
     * @throws \RuntimeException When the HTTP header is not parseable
     */
    public function getDate(string $key, ?\DateTimeInterface $default = null): ?\DateTimeImmutable
    {}

    /**
     * Adds a custom Cache-Control directive.
     */
    public function addCacheControlDirective(string $key, bool|string $value = true): void
    {}

    /**
     * Returns true if the Cache-Control directive is defined.
     */
    public function hasCacheControlDirective(string $key): bool
    {}

    /**
     * Returns a Cache-Control directive value by name.
     */
    public function getCacheControlDirective(string $key): bool|string|null
    {}

    /**
     * Removes a Cache-Control directive.
     */
    public function removeCacheControlDirective(string $key): void
    {}

    /**
     * Returns an iterator for headers.
     *
     * @return \ArrayIterator<string, list<string|null>>
     */
    public function getIterator(): \ArrayIterator
    {}

    /**
     * Returns the number of headers.
     */
    public function count(): int
    {}

    protected function getCacheControlHeader(): string
    {}

    /**
     * Parses a Cache-Control HTTP header.
     */
    protected function parseCacheControl(string $header): array
    {}
}
