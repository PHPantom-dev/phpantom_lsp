<?php

declare(strict_types=1);

namespace Doctrine\ORM;

use Doctrine\DBAL\LockMode;
use Doctrine\DBAL\Result;
use Doctrine\DBAL\Types\Type;
use Doctrine\Deprecations\Deprecation;
use Doctrine\ORM\Mapping\ClassMetadata;
use Doctrine\ORM\Query\AST\DeleteStatement;
use Doctrine\ORM\Query\AST\SelectStatement;
use Doctrine\ORM\Query\AST\UpdateStatement;
use Doctrine\ORM\Query\Exec\AbstractSqlExecutor;
use Doctrine\ORM\Query\Exec\SqlFinalizer;
use Doctrine\ORM\Query\OutputWalker;
use Doctrine\ORM\Query\Parameter;
use Doctrine\ORM\Query\ParameterTypeInferer;
use Doctrine\ORM\Query\Parser;
use Doctrine\ORM\Query\ParserResult;
use Doctrine\ORM\Query\QueryException;
use Doctrine\ORM\Query\ResultSetMapping;
use Doctrine\ORM\Utility\HierarchyDiscriminatorResolver;
use Psr\Cache\CacheItemPoolInterface;

use function array_keys;
use function array_values;
use function assert;
use function count;
use function get_debug_type;
use function in_array;
use function is_a;
use function ksort;
use function md5;
use function reset;
use function serialize;
use function sha1;
use function stripos;

/**
 * A Query object represents a DQL query.
 *
 * @final
 */
class Query extends AbstractQuery
{
    /**
     * A query object is in CLEAN state when it has NO unparsed/unprocessed DQL parts.
     */
    public const STATE_CLEAN = 1;

    /**
     * A query object is in state DIRTY when it has DQL parts that have not yet been
     * parsed/processed. This is automatically defined as DIRTY when addDqlQueryPart
     * is called.
     */
    public const STATE_DIRTY = 2;

    /* Query HINTS */

    /**
     * The refresh hint turns any query into a refresh query with the result that
     * any local changes in entities are overridden with the fetched values.
     */
    public const HINT_REFRESH = 'doctrine.refresh';

    public const HINT_CACHE_ENABLED = 'doctrine.cache.enabled';

    public const HINT_CACHE_EVICT = 'doctrine.cache.evict';

    /**
     * Internal hint: is set to the proxy entity that is currently triggered for loading
     */
    public const HINT_REFRESH_ENTITY = 'doctrine.refresh.entity';

    /**
     * The forcePartialLoad query hint forces a particular query to return
     * partial objects.
     *
     * @todo Rename: HINT_OPTIMIZE
     */
    public const HINT_FORCE_PARTIAL_LOAD = 'doctrine.forcePartialLoad';

    /**
     * The includeMetaColumns query hint causes meta columns like foreign keys and
     * discriminator columns to be selected and returned as part of the query result.
     *
     * This hint does only apply to non-object queries.
     */
    public const HINT_INCLUDE_META_COLUMNS = 'doctrine.includeMetaColumns';

    /**
     * An array of class names that implement \Doctrine\ORM\Query\TreeWalker and
     * are iterated and executed after the DQL has been parsed into an AST.
     */
    public const HINT_CUSTOM_TREE_WALKERS = 'doctrine.customTreeWalkers';

    /**
     * A string with a class name that implements \Doctrine\ORM\Query\TreeWalker
     * and is used for generating the target SQL from any DQL AST tree.
     */
    public const HINT_CUSTOM_OUTPUT_WALKER = 'doctrine.customOutputWalker';

    /**
     * Marks queries as creating only read only objects.
     *
     * If the object retrieved from the query is already in the identity map
     * then it does not get marked as read only if it wasn't already.
     */
    public const HINT_READ_ONLY = 'doctrine.readOnly';

    public const HINT_INTERNAL_ITERATION = 'doctrine.internal.iteration';

    public const HINT_LOCK_MODE = 'doctrine.lockMode';

    /**
     * The current state of this query.
     *
     * @phpstan-var self::STATE_*
     */
    private int $state = self::STATE_DIRTY;

    /**
     * A snapshot of the parameter types the query was parsed with.
     *
     * @var array<string,Type>
     */
    private array $parsedTypes = [];

    /**
     * Cached DQL query.
     */
    private string|null $dql = null;

    /**
     * The parser result that holds DQL => SQL information.
     */
    private ParserResult $parserResult;

    /**
     * The first result to return (the "offset").
     */
    private int $firstResult = 0;

    /**
     * The maximum number of results to return (the "limit").
     */
    private int|null $maxResults = null;

    /**
     * The cache driver used for caching queries.
     */
    private CacheItemPoolInterface|null $queryCache = null;

    /**
     * Whether or not expire the query cache.
     */
    private bool $expireQueryCache = false;

    /**
     * The query cache lifetime.
     */
    private int|null $queryCacheTTL = null;

    /**
     * Whether to use a query cache, if available. Defaults to TRUE.
     */
    private bool $useQueryCache = true;

    /**
     * Gets the SQL query/queries that correspond to this DQL query.
     *
     * @return list<string>|string The built sql query or an array of all sql queries.
     */
    public function getSQL(): string|array
    {}

    /**
     * Returns the corresponding AST for this DQL query.
     */
    public function getAST(): SelectStatement|UpdateStatement|DeleteStatement
    {}

    protected function getResultSetMapping(): ResultSetMapping
    {}

    /**
     * Parses the DQL query, if necessary, and stores the parser result.
     *
     * Note: Populates $this->_parserResult as a side-effect.
     */
    private function parse(): ParserResult
    {}

    protected function _doExecute(): Result|int
    {}

    /**
     * @param array<string,mixed> $sqlParams
     * @param array<string,Type>  $types
     * @param array<string,mixed> $connectionParams
     */
    private function evictResultSetCache(
        AbstractSqlExecutor $executor,
        array $sqlParams,
        array $types,
        array $connectionParams,
    ): void {}

    /**
     * Evict entity cache region
     */
    private function evictEntityCacheRegion(): void
    {}

    /**
     * Processes query parameter mappings.
     *
     * @param array<list<int>> $paramMappings
     *
     * @return mixed[][]
     * @phpstan-return array{0: list<mixed>, 1: array}
     *
     * @throws Query\QueryException
     */
    private function processParameterMappings(array $paramMappings): array
    {}

    /**
     * @return mixed[] tuple of (value, type)
     * @phpstan-return array{0: mixed, 1: mixed}
     */
    private function resolveParameterValue(Parameter $parameter): array
    {}

    /**
     * Defines a cache driver to be used for caching queries.
     *
     * @return $this
     */
    public function setQueryCache(CacheItemPoolInterface|null $queryCache): self
    {}

    /**
     * Defines whether the query should make use of a query cache, if available.
     *
     * @return $this
     */
    public function useQueryCache(bool $bool): self
    {}

    /**
     * Defines how long the query cache will be active before expire.
     *
     * @param int|null $timeToLive How long the cache entry is valid.
     *
     * @return $this
     */
    public function setQueryCacheLifetime(int|null $timeToLive): self
    {}

    /**
     * Retrieves the lifetime of resultset cache.
     */
    public function getQueryCacheLifetime(): int|null
    {}

    /**
     * Defines if the query cache is active or not.
     *
     * @return $this
     */
    public function expireQueryCache(bool $expire = true): self
    {}

    /**
     * Retrieves if the query cache is active or not.
     */
    public function getExpireQueryCache(): bool
    {}

    public function free(): void
    {}

    /**
     * Sets a DQL query string.
     */
    public function setDQL(string $dqlQuery): self
    {}

    /**
     * Returns the DQL query that is represented by this query object.
     */
    public function getDQL(): string|null
    {}

    /**
     * Returns the state of this query object
     * By default the type is Doctrine_ORM_Query_Abstract::STATE_CLEAN but if it appears any unprocessed DQL
     * part, it is switched to Doctrine_ORM_Query_Abstract::STATE_DIRTY.
     *
     * @see AbstractQuery::STATE_CLEAN
     * @see AbstractQuery::STATE_DIRTY
     *
     * @return int The query state.
     * @phpstan-return self::STATE_* The query state.
     */
    public function getState(): int
    {}

    /**
     * Method to check if an arbitrary piece of DQL exists
     *
     * @param string $dql Arbitrary piece of DQL to check for.
     */
    public function contains(string $dql): bool
    {}

    /**
     * Sets the position of the first result to retrieve (the "offset").
     *
     * @param int $firstResult The first result to return.
     *
     * @return $this
     */
    public function setFirstResult(int $firstResult): self
    {}

    /**
     * Gets the position of the first result the query object was set to retrieve (the "offset").
     * Returns 0 if {@link setFirstResult} was not applied to this query.
     *
     * @return int The position of the first result.
     */
    public function getFirstResult(): int
    {}

    /**
     * Sets the maximum number of results to retrieve (the "limit").
     *
     * @return $this
     */
    public function setMaxResults(int|null $maxResults): self
    {}

    /**
     * Gets the maximum number of results the query object was set to retrieve (the "limit").
     * Returns NULL if {@link setMaxResults} was not applied to this query.
     *
     * @return int|null Maximum number of results.
     */
    public function getMaxResults(): int|null
    {}

    /** {@inheritDoc} */
    public function toIterable(iterable $parameters = [], $hydrationMode = self::HYDRATE_OBJECT): iterable
    {}

    public function setHint(string $name, mixed $value): static
    {}

    public function setHydrationMode(string|int $hydrationMode): static
    {}

    /**
     * Set the lock mode for this Query.
     *
     * @see \Doctrine\DBAL\LockMode
     *
     * @phpstan-param LockMode::* $lockMode
     *
     * @return $this
     *
     * @throws TransactionRequiredException
     */
    public function setLockMode(LockMode|int $lockMode): self
    {}

    /**
     * Get the current lock mode for this query.
     *
     * @return LockMode|int|null The current lock mode of this query or NULL if no specific lock mode is set.
     * @phpstan-return LockMode::*|null
     */
    public function getLockMode(): LockMode|int|null
    {}

    /**
     * Generate a cache id for the query cache - reusing the Result-Cache-Id generator.
     */
    protected function getQueryCacheId(): string
    {}

    protected function getHash(): string
    {}

    /**
     * Cleanup Query resource when clone is called.
     */
    public function __clone()
    {}

    private function getSqlExecutor(): AbstractSqlExecutor
    {}
}
