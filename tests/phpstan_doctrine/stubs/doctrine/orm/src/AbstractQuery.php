<?php

declare(strict_types=1);

namespace Doctrine\ORM;

use BackedEnum;
use Doctrine\Common\Collections\ArrayCollection;
use Doctrine\Common\Collections\Collection;
use Doctrine\DBAL\ArrayParameterType;
use Doctrine\DBAL\Cache\QueryCacheProfile;
use Doctrine\DBAL\ParameterType;
use Doctrine\DBAL\Result;
use Doctrine\ORM\Cache\Logging\CacheLogger;
use Doctrine\ORM\Cache\QueryCacheKey;
use Doctrine\ORM\Cache\TimestampCacheKey;
use Doctrine\ORM\Mapping\ClassMetadata;
use Doctrine\ORM\Mapping\MappingException as ORMMappingException;
use Doctrine\ORM\Proxy\DefaultProxyClassNameResolver;
use Doctrine\ORM\Query\Parameter;
use Doctrine\ORM\Query\ResultSetMapping;
use Doctrine\Persistence\Mapping\MappingException;
use LogicException;
use Psr\Cache\CacheItemPoolInterface;
use Traversable;

use function array_map;
use function array_shift;
use function assert;
use function count;
use function is_array;
use function is_numeric;
use function is_object;
use function is_scalar;
use function is_string;
use function iterator_to_array;
use function ksort;
use function reset;
use function serialize;
use function sha1;

/**
 * Base contract for ORM queries. Base class for Query and NativeQuery.
 *
 * @link    www.doctrine-project.org
 */
abstract class AbstractQuery
{
    /* Hydration mode constants */

    /**
     * Hydrates an object graph. This is the default behavior.
     */
    public const HYDRATE_OBJECT = 1;

    /**
     * Hydrates an array graph.
     */
    public const HYDRATE_ARRAY = 2;

    /**
     * Hydrates a flat, rectangular result set with scalar values.
     */
    public const HYDRATE_SCALAR = 3;

    /**
     * Hydrates a single scalar value.
     */
    public const HYDRATE_SINGLE_SCALAR = 4;

    /**
     * Very simple object hydrator (optimized for performance).
     */
    public const HYDRATE_SIMPLEOBJECT = 5;

    /**
     * Hydrates scalar column value.
     */
    public const HYDRATE_SCALAR_COLUMN = 6;

    /**
     * The parameter map of this query.
     *
     * @var ArrayCollection|Parameter[]
     * @phpstan-var ArrayCollection<int, Parameter>
     */
    protected ArrayCollection $parameters;

    /**
     * The user-specified ResultSetMapping to use.
     */
    protected ResultSetMapping|null $resultSetMapping = null;

    /**
     * The map of query hints.
     *
     * @phpstan-var array<string, mixed>
     */
    protected array $hints = [];

    /**
     * The hydration mode.
     *
     * @phpstan-var string|AbstractQuery::HYDRATE_*
     */
    protected string|int $hydrationMode = self::HYDRATE_OBJECT;

    protected QueryCacheProfile|null $queryCacheProfile = null;

    /**
     * Whether or not expire the result cache.
     */
    protected bool $expireResultCache = false;

    protected QueryCacheProfile|null $hydrationCacheProfile = null;

    /**
     * Whether to use second level cache, if available.
     */
    protected bool $cacheable = false;

    protected bool $hasCache = false;

    /**
     * Second level cache region name.
     */
    protected string|null $cacheRegion = null;

    /**
     * Second level query cache mode.
     *
     * @phpstan-var Cache::MODE_*|null
     */
    protected int|null $cacheMode = null;

    protected CacheLogger|null $cacheLogger = null;

    protected int $lifetime = 0;

    /**
     * Initializes a new instance of a class derived from <tt>AbstractQuery</tt>.
     */
    public function __construct(
        /**
         * The entity manager used by this query object.
         */
        protected EntityManagerInterface $em,
    ) {}

    /**
     * Enable/disable second level query (result) caching for this query.
     *
     * @return $this
     */
    public function setCacheable(bool $cacheable): static
    {}

    /** @return bool TRUE if the query results are enabled for second level cache, FALSE otherwise. */
    public function isCacheable(): bool
    {}

    /** @return $this */
    public function setCacheRegion(string $cacheRegion): static
    {}

    /**
     * Obtain the name of the second level query cache region in which query results will be stored
     *
     * @return string|null The cache region name; NULL indicates the default region.
     */
    public function getCacheRegion(): string|null
    {}

    /** @return bool TRUE if the query cache and second level cache are enabled, FALSE otherwise. */
    protected function isCacheEnabled(): bool
    {}

    public function getLifetime(): int
    {}

    /**
     * Sets the life-time for this query into second level cache.
     *
     * @return $this
     */
    public function setLifetime(int $lifetime): static
    {}

    /** @phpstan-return Cache::MODE_*|null */
    public function getCacheMode(): int|null
    {}

    /**
     * @phpstan-param Cache::MODE_* $cacheMode
     *
     * @return $this
     */
    public function setCacheMode(int $cacheMode): static
    {}

    /**
     * Gets the SQL query that corresponds to this query object.
     * The returned SQL syntax depends on the connection driver that is used
     * by this query object at the time of this method call.
     *
     * @return list<string>|string SQL query
     */
    abstract public function getSQL(): string|array;

    /**
     * Retrieves the associated EntityManager of this Query instance.
     */
    public function getEntityManager(): EntityManagerInterface
    {}

    /**
     * Frees the resources used by the query object.
     *
     * Resets Parameters, Parameter Types and Query Hints.
     */
    public function free(): void
    {}

    /**
     * Get all defined parameters.
     *
     * @phpstan-return ArrayCollection<int, Parameter>
     */
    public function getParameters(): ArrayCollection
    {}

    /**
     * Gets a query parameter.
     *
     * @param int|string $key The key (index or name) of the bound parameter.
     *
     * @return Parameter|null The value of the bound parameter, or NULL if not available.
     */
    public function getParameter(int|string $key): Parameter|null
    {}

    /**
     * Sets a collection of query parameters.
     *
     * @param ArrayCollection|mixed[] $parameters
     * @phpstan-param ArrayCollection<int, Parameter>|mixed[] $parameters
     *
     * @return $this
     */
    public function setParameters(ArrayCollection|array $parameters): static
    {}

    /**
     * Sets a query parameter.
     *
     * @param string|int                                       $key   The parameter position or name.
     * @param mixed                                            $value The parameter value.
     * @param ParameterType|ArrayParameterType|string|int|null $type  The parameter type. If specified, the given value
     *                                                                will be run through the type conversion of this
     *                                                                type. This is usually not needed for strings and
     *                                                                numeric types.
     *
     * @return $this
     */
    public function setParameter(string|int $key, mixed $value, ParameterType|ArrayParameterType|string|int|null $type = null): static
    {}

    /**
     * Processes an individual parameter value.
     *
     * @throws ORMInvalidArgumentException
     */
    public function processParameterValue(mixed $value): mixed
    {}

    /**
     * If no mapping is detected, trying to resolve the value as a Traversable
     */
    private function potentiallyProcessIterable(mixed $value): mixed
    {}

    /**
     * Process a parameter value which was previously identified as an array
     *
     * @param mixed[] $value
     *
     * @return mixed[]
     */
    private function processArrayParameterValue(array $value): array
    {}

    /**
     * Sets the ResultSetMapping that should be used for hydration.
     *
     * @return $this
     */
    public function setResultSetMapping(ResultSetMapping $rsm): static
    {}

    /**
     * Gets the ResultSetMapping used for hydration.
     */
    protected function getResultSetMapping(): ResultSetMapping|null
    {}

    /**
     * Allows to translate entity namespaces to full qualified names.
     */
    private function translateNamespaces(ResultSetMapping $rsm): void
    {}

    /**
     * Set a cache profile for hydration caching.
     *
     * If no result cache driver is set in the QueryCacheProfile, the default
     * result cache driver is used from the configuration.
     *
     * Important: Hydration caching does NOT register entities in the
     * UnitOfWork when retrieved from the cache. Never use result cached
     * entities for requests that also flush the EntityManager. If you want
     * some form of caching with UnitOfWork registration you should use
     * {@see AbstractQuery::setResultCacheProfile()}.
     *
     * @return $this
     *
     * @example
     * $lifetime = 100;
     * $resultKey = "abc";
     * $query->setHydrationCacheProfile(new QueryCacheProfile());
     * $query->setHydrationCacheProfile(new QueryCacheProfile($lifetime, $resultKey));
     */
    public function setHydrationCacheProfile(QueryCacheProfile|null $profile): static
    {}

    public function getHydrationCacheProfile(): QueryCacheProfile|null
    {}

    /**
     * Set a cache profile for the result cache.
     *
     * If no result cache driver is set in the QueryCacheProfile, the default
     * result cache driver is used from the configuration.
     *
     * @return $this
     */
    public function setResultCacheProfile(QueryCacheProfile|null $profile): static
    {}

    /**
     * Defines a cache driver to be used for caching result sets and implicitly enables caching.
     */
    public function setResultCache(CacheItemPoolInterface|null $resultCache): static
    {}

    /**
     * Enables caching of the results of this query, for given or default amount of seconds
     * and optionally specifies which ID to use for the cache entry.
     *
     * @param int|null    $lifetime      How long the cache entry is valid, in seconds.
     * @param string|null $resultCacheId ID to use for the cache entry.
     *
     * @return $this
     */
    public function enableResultCache(int|null $lifetime = null, string|null $resultCacheId = null): static
    {}

    /**
     * Disables caching of the results of this query.
     *
     * @return $this
     */
    public function disableResultCache(): static
    {}

    /**
     * Defines how long the result cache will be active before expire.
     *
     * @param int|null $lifetime How long the cache entry is valid, in seconds.
     *
     * @return $this
     */
    public function setResultCacheLifetime(int|null $lifetime): static
    {}

    /**
     * Defines if the result cache is active or not.
     *
     * @param bool $expire Whether or not to force resultset cache expiration.
     *
     * @return $this
     */
    public function expireResultCache(bool $expire = true): static
    {}

    /**
     * Retrieves if the resultset cache is active or not.
     */
    public function getExpireResultCache(): bool
    {}

    public function getQueryCacheProfile(): QueryCacheProfile|null
    {}

    /**
     * Change the default fetch mode of an association for this query.
     *
     * @param class-string $class
     * @phpstan-param Mapping\ClassMetadata::FETCH_EAGER|Mapping\ClassMetadata::FETCH_LAZY $fetchMode
     */
    public function setFetchMode(string $class, string $assocName, int $fetchMode): static
    {}

    /**
     * Defines the processing mode to be used during hydration / result set transformation.
     *
     * @param string|int $hydrationMode Doctrine processing mode to be used during hydration process.
     *                                  One of the Query::HYDRATE_* constants.
     * @phpstan-param string|AbstractQuery::HYDRATE_* $hydrationMode
     *
     * @return $this
     */
    public function setHydrationMode(string|int $hydrationMode): static
    {}

    /**
     * Gets the hydration mode currently used by the query.
     *
     * @phpstan-return string|AbstractQuery::HYDRATE_*
     */
    public function getHydrationMode(): string|int
    {}

    /**
     * Gets the list of results for the query.
     *
     * Alias for execute(null, $hydrationMode = HYDRATE_OBJECT).
     *
     * @phpstan-param string|AbstractQuery::HYDRATE_* $hydrationMode
     */
    public function getResult(string|int $hydrationMode = self::HYDRATE_OBJECT): mixed
    {}

    /**
     * Gets the array of results for the query.
     *
     * Alias for execute(null, HYDRATE_ARRAY).
     *
     * @return mixed[]
     */
    public function getArrayResult(): array
    {}

    /**
     * Gets one-dimensional array of results for the query.
     *
     * Alias for execute(null, HYDRATE_SCALAR_COLUMN).
     *
     * @return mixed[]
     */
    public function getSingleColumnResult(): array
    {}

    /**
     * Gets the scalar results for the query.
     *
     * Alias for execute(null, HYDRATE_SCALAR).
     *
     * @return mixed[]
     */
    public function getScalarResult(): array
    {}

    /**
     * Get exactly one result or null.
     *
     * @phpstan-param string|AbstractQuery::HYDRATE_*|null $hydrationMode
     *
     * @throws NonUniqueResultException
     */
    public function getOneOrNullResult(string|int|null $hydrationMode = null): mixed
    {}

    /**
     * Gets the single result of the query.
     *
     * Enforces the presence as well as the uniqueness of the result.
     *
     * If the result is not unique, a NonUniqueResultException is thrown.
     * If there is no result, a NoResultException is thrown.
     *
     * @phpstan-param string|AbstractQuery::HYDRATE_*|null $hydrationMode
     *
     * @throws NonUniqueResultException If the query result is not unique.
     * @throws NoResultException        If the query returned no result.
     */
    public function getSingleResult(string|int|null $hydrationMode = null): mixed
    {}

    /**
     * Gets the single scalar result of the query.
     *
     * Alias for getSingleResult(HYDRATE_SINGLE_SCALAR).
     *
     * @return bool|float|int|string|null The scalar result.
     *
     * @throws NoResultException        If the query returned no result.
     * @throws NonUniqueResultException If the query result is not unique.
     */
    public function getSingleScalarResult(): mixed
    {}

    /**
     * Sets a query hint. If the hint name is not recognized, it is silently ignored.
     *
     * @return $this
     */
    public function setHint(string $name, mixed $value): static
    {}

    /**
     * Gets the value of a query hint. If the hint name is not recognized, FALSE is returned.
     *
     * @return mixed The value of the hint or FALSE, if the hint name is not recognized.
     */
    public function getHint(string $name): mixed
    {}

    public function hasHint(string $name): bool
    {}

    /**
     * Return the key value map of query hints that are currently set.
     *
     * @return array<string,mixed>
     */
    public function getHints(): array
    {}

    /**
     * Executes the query and returns an iterable that can be used to incrementally
     * iterate over the result.
     *
     * @phpstan-param ArrayCollection<int, Parameter>|mixed[] $parameters
     * @phpstan-param string|AbstractQuery::HYDRATE_*|null    $hydrationMode
     *
     * @return iterable<mixed>
     */
    public function toIterable(
        ArrayCollection|array $parameters = [],
        string|int|null $hydrationMode = null,
    ): iterable {}

    /**
     * Executes the query.
     *
     * @phpstan-param ArrayCollection<int, Parameter>|mixed[]|null $parameters
     * @phpstan-param string|AbstractQuery::HYDRATE_*|null         $hydrationMode
     */
    public function execute(
        ArrayCollection|array|null $parameters = null,
        string|int|null $hydrationMode = null,
    ): mixed {}

    /**
     * Execute query ignoring second level cache.
     *
     * @phpstan-param ArrayCollection<int, Parameter>|mixed[]|null $parameters
     * @phpstan-param string|AbstractQuery::HYDRATE_*|null         $hydrationMode
     */
    private function executeIgnoreQueryCache(
        ArrayCollection|array|null $parameters = null,
        string|int|null $hydrationMode = null,
    ): mixed {}

    private function getHydrationCache(): CacheItemPoolInterface
    {}

    /**
     * Load from second level cache or executes the query and put into cache.
     *
     * @phpstan-param ArrayCollection<int, Parameter>|mixed[]|null $parameters
     * @phpstan-param string|AbstractQuery::HYDRATE_*|null         $hydrationMode
     */
    private function executeUsingQueryCache(
        ArrayCollection|array|null $parameters = null,
        string|int|null $hydrationMode = null,
    ): mixed {}

    private function getTimestampKey(): TimestampCacheKey|null
    {}

    /**
     * Get the result cache id to use to store the result set cache entry.
     * Will return the configured id if it exists otherwise a hash will be
     * automatically generated for you.
     *
     * @return string[] ($key, $hash)
     * @phpstan-return array{string, string} ($key, $hash)
     */
    protected function getHydrationCacheId(): array
    {}

    /**
     * Set the result cache id to use to store the result set cache entry.
     * If this is not explicitly set by the developer then a hash is automatically
     * generated for you.
     */
    public function setResultCacheId(string|null $id): static
    {}

    /**
     * Executes the query and returns the resulting Statement object.
     *
     * @return Result|int The executed database statement that holds
     *                    the results, or an integer indicating how
     *                    many rows were affected.
     */
    abstract protected function _doExecute(): Result|int;

    /**
     * Cleanup Query resource when clone is called.
     */
    public function __clone()
    {}

    /**
     * Generates a string of currently query to use for the cache second level cache.
     */
    protected function getHash(): string
    {}
}
