<?php

declare(strict_types=1);

namespace Doctrine\ORM;

use Doctrine\Common\Collections\ArrayCollection;
use Doctrine\Common\Collections\Criteria;
use Doctrine\DBAL\ArrayParameterType;
use Doctrine\DBAL\ParameterType;
use Doctrine\Deprecations\Deprecation;
use Doctrine\ORM\Cache\Persister\CompatOrderings;
use Doctrine\ORM\Internal\NoUnknownNamedArguments;
use Doctrine\ORM\Query\Expr;
use Doctrine\ORM\Query\Parameter;
use Doctrine\ORM\Query\QueryExpressionVisitor;
use InvalidArgumentException;
use RuntimeException;
use SortDirection;
use Stringable;

use function array_keys;
use function array_unshift;
use function assert;
use function count;
use function func_num_args;
use function get_debug_type;
use function implode;
use function in_array;
use function is_array;
use function is_numeric;
use function is_object;
use function is_string;
use function key;
use function reset;
use function sprintf;
use function str_starts_with;
use function strpos;
use function strrpos;
use function strtolower;
use function substr;

/**
 * This class is responsible for building DQL query strings via an object oriented
 * PHP interface.
 */
class QueryBuilder implements Stringable
{
    use CompatOrderings;
    use NoUnknownNamedArguments;

    /**
     * The array of DQL parts collected.
     *
     * @phpstan-var array<string, mixed>
     */
    private array $dqlParts = [
        'distinct' => false,
        'select'  => [],
        'from'    => [],
        'join'    => [],
        'set'     => [],
        'where'   => null,
        'groupBy' => [],
        'having'  => null,
        'orderBy' => [],
    ];

    private QueryType $type = QueryType::Select;

    /**
     * The complete DQL string for this query.
     */
    private string|null $dql = null;

    /**
     * The query parameters.
     *
     * @phpstan-var ArrayCollection<int, Parameter>
     */
    private ArrayCollection $parameters;

    /**
     * The index of the first result to retrieve.
     */
    private int $firstResult = 0;

    /**
     * The maximum number of results to retrieve.
     */
    private int|null $maxResults = null;

    /**
     * Keeps root entity alias names for join entities.
     *
     * @phpstan-var array<string, string>
     */
    private array $joinRootAliases = [];

    /**
     * Whether to use second level cache, if available.
     */
    protected bool $cacheable = false;

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

    protected int $lifetime = 0;

    /**
     * The counter of bound parameters.
     *
     * @var int<0, max>
     */
    private int $boundCounter = 0;

    /**
     * The hints to set on the query.
     *
     * @var array<string, string|int|bool|iterable<mixed>|object>
     */
    private array $hints = [];

    /**
     * Initializes a new <tt>QueryBuilder</tt> that uses the given <tt>EntityManager</tt>.
     *
     * @param EntityManagerInterface $em The EntityManager to use.
     */
    public function __construct(
        private readonly EntityManagerInterface $em,
    ) {}

    final protected function getType(): QueryType
    {}

    /**
     * Gets an ExpressionBuilder used for object-oriented construction of query expressions.
     * This producer method is intended for convenient inline usage. Example:
     *
     * <code>
     *     $qb = $em->createQueryBuilder();
     *     $qb
     *         ->select('u')
     *         ->from('User', 'u')
     *         ->where($qb->expr()->eq('u.id', 1));
     * </code>
     *
     * For more complex expression construction, consider storing the expression
     * builder object in a local variable.
     */
    public function expr(): Expr
    {}

    /**
     * Enable/disable second level query (result) caching for this query.
     *
     * @return $this
     */
    public function setCacheable(bool $cacheable): static
    {}

    /**
     * Are the query results enabled for second level cache?
     */
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

    public function getLifetime(): int
    {}

    /**
     * Sets the life-time for this query into second level cache.
     *
     * @return $this
     */
    public function setLifetime(int $lifetime): static
    {}

    /** @return array<string, string|int|bool|iterable<mixed>|object> */
    public function getHints(): array
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
     * Adds hints for the query.
     *
     * @return $this
     */
    public function setHint(string $name, mixed $value): static
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
     * Gets the associated EntityManager for this query builder.
     */
    public function getEntityManager(): EntityManagerInterface
    {}

    /**
     * Gets the complete DQL string formed by the current specifications of this QueryBuilder.
     *
     * <code>
     *     $qb = $em->createQueryBuilder()
     *         ->select('u')
     *         ->from('User', 'u');
     *     echo $qb->getDql(); // SELECT u FROM User u
     * </code>
     */
    public function getDQL(): string
    {}

    /**
     * Constructs a Query instance from the current specifications of the builder.
     *
     * <code>
     *     $qb = $em->createQueryBuilder()
     *         ->select('u')
     *         ->from('User', 'u');
     *     $q = $qb->getQuery();
     *     $results = $q->execute();
     * </code>
     */
    public function getQuery(): Query
    {}

    /**
     * Finds the root entity alias of the joined entity.
     *
     * @param string $alias       The alias of the new join entity
     * @param string $parentAlias The parent entity alias of the join relationship
     */
    private function findRootAlias(string $alias, string $parentAlias): string
    {}

    /**
     * Gets the FIRST root alias of the query. This is the first entity alias involved
     * in the construction of the query.
     *
     * <code>
     * $qb = $em->createQueryBuilder()
     *     ->select('u')
     *     ->from('User', 'u');
     *
     * echo $qb->getRootAlias(); // u
     * </code>
     *
     * @deprecated Please use $qb->getRootAliases() instead.
     *
     * @throws RuntimeException
     */
    public function getRootAlias(): string
    {}

    /**
     * Gets the root aliases of the query. This is the entity aliases involved
     * in the construction of the query.
     *
     * <code>
     *     $qb = $em->createQueryBuilder()
     *         ->select('u')
     *         ->from('User', 'u');
     *
     *     $qb->getRootAliases(); // array('u')
     * </code>
     *
     * @return string[]
     * @phpstan-return list<string>
     */
    public function getRootAliases(): array
    {}

    /**
     * Gets all the aliases that have been used in the query.
     * Including all select root aliases and join aliases
     *
     * <code>
     *     $qb = $em->createQueryBuilder()
     *         ->select('u')
     *         ->from('User', 'u')
     *         ->join('u.articles','a');
     *
     *     $qb->getAllAliases(); // array('u','a')
     * </code>
     *
     * @return string[]
     * @phpstan-return list<string>
     */
    public function getAllAliases(): array
    {}

    /**
     * Gets the root entities of the query. This is the entity classes involved
     * in the construction of the query.
     *
     * <code>
     *     $qb = $em->createQueryBuilder()
     *         ->select('u')
     *         ->from('User', 'u');
     *
     *     $qb->getRootEntities(); // array('User')
     * </code>
     *
     * @return string[]
     * @phpstan-return list<class-string>
     */
    public function getRootEntities(): array
    {}

    /**
     * Sets a query parameter for the query being constructed.
     *
     * <code>
     *     $qb = $em->createQueryBuilder()
     *         ->select('u')
     *         ->from('User', 'u')
     *         ->where('u.id = :user_id')
     *         ->setParameter('user_id', 1);
     * </code>
     *
     * @param string|int                                       $key  The parameter position or name.
     * @param ParameterType|ArrayParameterType|string|int|null $type ParameterType::*, ArrayParameterType::* or \Doctrine\DBAL\Types\Type::* constant
     *
     * @return $this
     */
    public function setParameter(string|int $key, mixed $value, ParameterType|ArrayParameterType|string|int|null $type = null): static
    {}

    /**
     * Sets a collection of query parameters for the query being constructed.
     *
     * <code>
     *     $qb = $em->createQueryBuilder()
     *         ->select('u')
     *         ->from('User', 'u')
     *         ->where('u.id = :user_id1 OR u.id = :user_id2')
     *         ->setParameters(new ArrayCollection(array(
     *             new Parameter('user_id1', 1),
     *             new Parameter('user_id2', 2)
     *        )));
     * </code>
     *
     * @phpstan-param ArrayCollection<int, Parameter> $parameters
     *
     * @return $this
     */
    public function setParameters(ArrayCollection $parameters): static
    {}

    /**
     * Gets all defined query parameters for the query being constructed.
     *
     * @phpstan-return ArrayCollection<int, Parameter>
     */
    public function getParameters(): ArrayCollection
    {}

    /**
     * Gets a (previously set) query parameter of the query being constructed.
     */
    public function getParameter(string|int $key): Parameter|null
    {}

    /**
     * Sets the position of the first result to retrieve (the "offset").
     *
     * @return $this
     */
    public function setFirstResult(int|null $firstResult): static
    {}

    /**
     * Gets the position of the first result the query object was set to retrieve (the "offset").
     */
    public function getFirstResult(): int
    {}

    /**
     * Sets the maximum number of results to retrieve (the "limit").
     *
     * @return $this
     */
    public function setMaxResults(int|null $maxResults): static
    {}

    /**
     * Gets the maximum number of results the query object was set to retrieve (the "limit").
     * Returns NULL if {@link setMaxResults} was not applied to this query builder.
     */
    public function getMaxResults(): int|null
    {}

    /**
     * Either appends to or replaces a single, generic query part.
     *
     * The available parts are: 'select', 'from', 'join', 'set', 'where',
     * 'groupBy', 'having' and 'orderBy'.
     *
     * @phpstan-param string|object|list<string>|array{join: array<int|string, object>} $dqlPart
     *
     * @return $this
     */
    public function add(string $dqlPartName, string|object|array $dqlPart, bool $append = false): static
    {}

    /**
     * Specifies an item that is to be returned in the query result.
     * Replaces any previously specified selections, if any.
     *
     * <code>
     *     $qb = $em->createQueryBuilder()
     *         ->select('u', 'p')
     *         ->from('User', 'u')
     *         ->leftJoin('u.Phonenumbers', 'p');
     * </code>
     *
     * @return $this
     */
    public function select(mixed ...$select): static
    {}

    /**
     * Adds a DISTINCT flag to this query.
     *
     * <code>
     *     $qb = $em->createQueryBuilder()
     *         ->select('u')
     *         ->distinct()
     *         ->from('User', 'u');
     * </code>
     *
     * @return $this
     */
    public function distinct(bool $flag = true): static
    {}

    /**
     * Adds an item that is to be returned in the query result.
     *
     * <code>
     *     $qb = $em->createQueryBuilder()
     *         ->select('u')
     *         ->addSelect('p')
     *         ->from('User', 'u')
     *         ->leftJoin('u.Phonenumbers', 'p');
     * </code>
     *
     * @return $this
     */
    public function addSelect(mixed ...$select): static
    {}

    /**
     * Turns the query being built into a bulk delete query that ranges over
     * a certain entity type.
     *
     * <code>
     *     $qb = $em->createQueryBuilder()
     *         ->delete('User', 'u')
     *         ->where('u.id = :user_id')
     *         ->setParameter('user_id', 1);
     * </code>
     *
     * @param class-string|null $delete The class/type whose instances are subject to the deletion.
     * @param string|null       $alias  The class/type alias used in the constructed query.
     *
     * @return $this
     */
    public function delete(string|null $delete = null, string|null $alias = null): static
    {}

    /**
     * Turns the query being built into a bulk update query that ranges over
     * a certain entity type.
     *
     * <code>
     *     $qb = $em->createQueryBuilder()
     *         ->update('User', 'u')
     *         ->set('u.password', '?1')
     *         ->where('u.id = ?2');
     * </code>
     *
     * @param class-string|null $update The class/type whose instances are subject to the update.
     * @param string|null       $alias  The class/type alias used in the constructed query.
     *
     * @return $this
     */
    public function update(string|null $update = null, string|null $alias = null): static
    {}

    /**
     * Creates and adds a query root corresponding to the entity identified by the given alias,
     * forming a cartesian product with any existing query roots.
     *
     * <code>
     *     $qb = $em->createQueryBuilder()
     *         ->select('u')
     *         ->from('User', 'u');
     * </code>
     *
     * @param class-string $from    The class name.
     * @param string       $alias   The alias of the class.
     * @param string|null  $indexBy The index for the from.
     *
     * @return $this
     */
    public function from(string $from, string $alias, string|null $indexBy = null): static
    {}

    /**
     * Updates a query root corresponding to an entity setting its index by. This method is intended to be used with
     * EntityRepository->createQueryBuilder(), which creates the initial FROM clause and do not allow you to update it
     * setting an index by.
     *
     * <code>
     *     $qb = $userRepository->createQueryBuilder('u')
     *         ->indexBy('u', 'u.id');
     *
     *     // Is equivalent to...
     *
     *     $qb = $em->createQueryBuilder()
     *         ->select('u')
     *         ->from('User', 'u', 'u.id');
     * </code>
     *
     * @return $this
     *
     * @throws Query\QueryException
     */
    public function indexBy(string $alias, string $indexBy): static
    {}

    /**
     * Creates and adds a join over an entity association to the query.
     *
     * The entities in the joined association will be fetched as part of the query
     * result if the alias used for the joined association is placed in the select
     * expressions.
     *
     * <code>
     *     $qb = $em->createQueryBuilder()
     *         ->select('u')
     *         ->from('User', 'u')
     *         ->join('u.Phonenumbers', 'p', Expr\Join::WITH, 'p.is_primary = 1');
     * </code>
     *
     * @phpstan-param Expr\Join::ON|Expr\Join::WITH|null $conditionType
     *
     * @return $this
     */
    public function join(
        string $join,
        string $alias,
        string|null $conditionType = null,
        string|Expr\Composite|Expr\Comparison|Expr\Func|null $condition = null,
        string|null $indexBy = null,
    ): static {}

    /**
     * Creates and adds a join over an entity association to the query.
     *
     * The entities in the joined association will be fetched as part of the query
     * result if the alias used for the joined association is placed in the select
     * expressions.
     *
     *     [php]
     *     $qb = $em->createQueryBuilder()
     *         ->select('u')
     *         ->from('User', 'u')
     *         ->innerJoin('u.Phonenumbers', 'p', Expr\Join::WITH, 'p.is_primary = 1');
     *
     * @phpstan-param Expr\Join::ON|Expr\Join::WITH|null $conditionType
     *
     * @return $this
     */
    public function innerJoin(
        string $join,
        string $alias,
        string|null $conditionType = null,
        string|Expr\Composite|Expr\Comparison|Expr\Func|null $condition = null,
        string|null $indexBy = null,
    ): static {}

    /**
     * Creates and adds a left join over an entity association to the query.
     *
     * The entities in the joined association will be fetched as part of the query
     * result if the alias used for the joined association is placed in the select
     * expressions.
     *
     * <code>
     *     $qb = $em->createQueryBuilder()
     *         ->select('u')
     *         ->from('User', 'u')
     *         ->leftJoin('u.Phonenumbers', 'p', Expr\Join::WITH, 'p.is_primary = 1');
     * </code>
     *
     * @phpstan-param Expr\Join::ON|Expr\Join::WITH|null $conditionType
     *
     * @return $this
     */
    public function leftJoin(
        string $join,
        string $alias,
        string|null $conditionType = null,
        string|Expr\Composite|Expr\Comparison|Expr\Func|null $condition = null,
        string|null $indexBy = null,
    ): static {}

    /**
     * Sets a new value for a field in a bulk update query.
     *
     * <code>
     *     $qb = $em->createQueryBuilder()
     *         ->update('User', 'u')
     *         ->set('u.password', '?1')
     *         ->where('u.id = ?2');
     * </code>
     *
     * @return $this
     */
    public function set(string $key, mixed $value): static
    {}

    /**
     * Specifies one or more restrictions to the query result.
     * Replaces any previously specified restrictions, if any.
     *
     * <code>
     *     $qb = $em->createQueryBuilder()
     *         ->select('u')
     *         ->from('User', 'u')
     *         ->where('u.id = ?');
     *
     *     // You can optionally programmatically build and/or expressions
     *     $qb = $em->createQueryBuilder();
     *
     *     $or = $qb->expr()->orX();
     *     $or->add($qb->expr()->eq('u.id', 1));
     *     $or->add($qb->expr()->eq('u.id', 2));
     *
     *     $qb->update('User', 'u')
     *         ->set('u.password', '?')
     *         ->where($or);
     * </code>
     *
     * @return $this
     */
    public function where(mixed ...$predicates): static
    {}

    /**
     * Adds one or more restrictions to the query results, forming a logical
     * conjunction with any previously specified restrictions.
     *
     * <code>
     *     $qb = $em->createQueryBuilder()
     *         ->select('u')
     *         ->from('User', 'u')
     *         ->where('u.username LIKE ?')
     *         ->andWhere('u.is_active = 1');
     * </code>
     *
     * @see where()
     *
     * @return $this
     */
    public function andWhere(mixed ...$where): static
    {}

    /**
     * Adds one or more restrictions to the query results, forming a logical
     * disjunction with any previously specified restrictions.
     *
     * <code>
     *     $qb = $em->createQueryBuilder()
     *         ->select('u')
     *         ->from('User', 'u')
     *         ->where('u.id = 1')
     *         ->orWhere('u.id = 2');
     * </code>
     *
     * @see where()
     *
     * @return $this
     */
    public function orWhere(mixed ...$where): static
    {}

    /**
     * Specifies a grouping over the results of the query.
     * Replaces any previously specified groupings, if any.
     *
     * <code>
     *     $qb = $em->createQueryBuilder()
     *         ->select('u')
     *         ->from('User', 'u')
     *         ->groupBy('u.id');
     * </code>
     *
     * @return $this
     */
    public function groupBy(string ...$groupBy): static
    {}

    /**
     * Adds a grouping expression to the query.
     *
     * <code>
     *     $qb = $em->createQueryBuilder()
     *         ->select('u')
     *         ->from('User', 'u')
     *         ->groupBy('u.lastLogin')
     *         ->addGroupBy('u.createdAt');
     * </code>
     *
     * @return $this
     */
    public function addGroupBy(string ...$groupBy): static
    {}

    /**
     * Specifies a restriction over the groups of the query.
     * Replaces any previous having restrictions, if any.
     *
     * @return $this
     */
    public function having(mixed ...$having): static
    {}

    /**
     * Adds a restriction over the groups of the query, forming a logical
     * conjunction with any existing having restrictions.
     *
     * @return $this
     */
    public function andHaving(mixed ...$having): static
    {}

    /**
     * Adds a restriction over the groups of the query, forming a logical
     * disjunction with any existing having restrictions.
     *
     * @return $this
     */
    public function orHaving(mixed ...$having): static
    {}

    /**
     * Specifies an ordering for the query results.
     * Replaces any previously specified orderings, if any.
     *
     * @return $this
     */
    public function orderBy(string|Expr\OrderBy $sort, SortDirection|string|null $order = null): static
    {}

    /**
     * Adds an ordering to the query results.
     *
     * @return $this
     */
    public function addOrderBy(string|Expr\OrderBy $sort, SortDirection|string|null $order = null): static
    {}

    /**
     * Adds criteria to the query.
     *
     * Adds where expressions with AND operator.
     * Adds orderings.
     * Overrides firstResult and maxResults if they're set.
     *
     * @return $this
     *
     * @throws Query\QueryException
     */
    public function addCriteria(Criteria $criteria): static
    {}

    /**
     * Gets a query part by its name.
     */
    public function getDQLPart(string $queryPartName): mixed
    {}

    /**
     * Gets all query parts.
     *
     * @phpstan-return array<string, mixed> $dqlParts
     */
    public function getDQLParts(): array
    {}

    private function getDQLForDelete(): string
    {}

    private function getDQLForUpdate(): string
    {}

    private function getDQLForSelect(): string
    {}

    /** @phpstan-param array<string, mixed> $options */
    private function getReducedDQLQueryPart(string $queryPartName, array $options = []): string
    {}

    /**
     * Resets DQL parts.
     *
     * @param string[]|null $parts
     * @phpstan-param list<string>|null $parts
     *
     * @return $this
     */
    public function resetDQLParts(array|null $parts = null): static
    {}

    /**
     * Resets single DQL part.
     *
     * @return $this
     */
    public function resetDQLPart(string $part): static
    {}

    /**
     * Creates a new named parameter and bind the value $value to it.
     *
     * The parameter $value specifies the value that you want to bind. If
     * $placeholder is not provided createNamedParameter() will automatically
     * create a placeholder for you. An automatic placeholder will be of the
     * name ':dcValue1', ':dcValue2' etc.
     *
     * Example:
     *  <code>
     *   $qb = $em->createQueryBuilder();
     *   $qb
     *      ->select('u')
     *      ->from('User', 'u')
     *      ->where('u.username = ' . $qb->createNamedParameter('Foo', Types::STRING))
     *      ->orWhere('u.username = ' . $qb->createNamedParameter('Bar', Types::STRING))
     *  </code>
     *
     * @param ParameterType|ArrayParameterType|string|int|null $type        ParameterType::*, ArrayParameterType::* or \Doctrine\DBAL\Types\Type::* constant
     * @param non-empty-string|null                            $placeholder The name to bind with. The string must start with a colon ':'.
     *
     * @return non-empty-string the placeholder name used.
     */
    public function createNamedParameter(mixed $value, ParameterType|ArrayParameterType|string|int|null $type = null, string|null $placeholder = null): string
    {}

    /**
     * Gets a string representation of this QueryBuilder which corresponds to
     * the final DQL query being constructed.
     */
    public function __toString(): string
    {}

    /**
     * Deep clones all expression objects in the DQL parts.
     *
     * @return void
     */
    public function __clone()
    {}

    private function getSortDirection(SortDirection|string|null $order, string $method): SortDirection
    {}
}
