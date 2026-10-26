<?php

/*
 * This file is part of the Symfony package.
 *
 * (c) Fabien Potencier <fabien@symfony.com>
 *
 * For the full copyright and license information, please view the LICENSE
 * file that was distributed with this source code.
 */

namespace Symfony\Component\Cache\Adapter;

use Psr\Cache\CacheItemInterface;
use Psr\Clock\ClockInterface;
use Psr\Log\LoggerAwareInterface;
use Psr\Log\LoggerAwareTrait;
use Symfony\Component\Cache\CacheItem;
use Symfony\Component\Cache\Exception\InvalidArgumentException;
use Symfony\Component\Cache\ResettableInterface;
use Symfony\Contracts\Cache\CacheInterface;
use Symfony\Contracts\Cache\NamespacedPoolInterface;

/**
 * An in-memory cache storage.
 *
 * Acts as a least-recently-used (LRU) storage when configured with a maximum number of items.
 *
 * @author Nicolas Grekas <p@tchwork.com>
 */
class ArrayAdapter implements AdapterInterface, CacheInterface, NamespacedPoolInterface, LoggerAwareInterface, ResettableInterface
{
    use LoggerAwareTrait;

    private array $values = [];
    private array $tags = [];
    private array $expiries = [];
    private array $subPools = [];
    private array $explicitExpiries = [];

    private static \Closure $createCacheItem;

    /**
     * @param bool $storeSerialized Disabling serialization can lead to cache corruptions when storing mutable values but increases performance otherwise
     */
    public function __construct(
        private int $defaultLifetime = 0,
        private bool $storeSerialized = true,
        private float $maxLifetime = 0,
        private int $maxItems = 0,
        private ?ClockInterface $clock = null,
    ) {}

    public function get(string $key, callable $callback, ?float $beta = null, ?array &$metadata = null): mixed
    {}

    public function delete(string $key): bool
    {}

    public function hasItem(mixed $key): bool
    {}

    public function getItem(mixed $key): CacheItem
    {}

    public function getItems(array $keys = []): iterable
    {}

    public function deleteItem(mixed $key): bool
    {}

    public function deleteItems(array $keys): bool
    {}

    public function save(CacheItemInterface $item): bool
    {}

    public function saveDeferred(CacheItemInterface $item): bool
    {}

    public function commit(): bool
    {}

    public function clear(string $prefix = ''): bool
    {}

    public function withSubNamespace(string $namespace): static
    {}

    /**
     * Returns all cached values, with cache miss as null.
     */
    public function getValues(): array
    {}

    public function reset(): void
    {}

    public function __clone()
    {}

    private function generateItems(array $keys, float $now, \Closure $f): \Generator
    {}

    private function freeze($value, string $key): string|int|float|bool|array|\UnitEnum|null
    {}

    private function unfreeze(string $key, bool &$isHit): mixed
    {}

    private function validateKeys(array $keys): bool
    {}

    private function getCurrentTime(): float
    {}
}
