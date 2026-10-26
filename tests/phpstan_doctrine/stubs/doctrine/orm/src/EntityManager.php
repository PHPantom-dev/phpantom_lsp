<?php

declare(strict_types=1);

namespace Doctrine\ORM;

use BackedEnum;
use DateTimeInterface;
use Doctrine\Common\EventManager;
use Doctrine\Common\EventManagerInterface;
use Doctrine\DBAL\Connection;
use Doctrine\DBAL\LockMode;
use Doctrine\Deprecations\Deprecation;
use Doctrine\ORM\Exception\EntityManagerClosed;
use Doctrine\ORM\Exception\InvalidHydrationMode;
use Doctrine\ORM\Exception\MissingIdentifierField;
use Doctrine\ORM\Exception\MissingMappingDriverImplementation;
use Doctrine\ORM\Exception\ORMException;
use Doctrine\ORM\Exception\UnrecognizedIdentifierFields;
use Doctrine\ORM\Internal\Hydration\AbstractHydrator;
use Doctrine\ORM\Mapping\ClassMetadata;
use Doctrine\ORM\Mapping\ClassMetadataFactory;
use Doctrine\ORM\Proxy\DefaultProxyClassNameResolver;
use Doctrine\ORM\Proxy\ProxyFactory;
use Doctrine\ORM\Query\Expr;
use Doctrine\ORM\Query\FilterCollection;
use Doctrine\ORM\Query\ResultSetMapping;
use Doctrine\ORM\Repository\RepositoryFactory;

use function array_keys;
use function is_array;
use function is_object;
use function ltrim;
use function method_exists;

/**
 * The EntityManager is the central access point to ORM functionality.
 *
 * It is a facade to all different ORM subsystems such as UnitOfWork,
 * Query Language and Repository API. The quickest way to obtain a fully
 * configured EntityManager is:
 *
 *     use Doctrine\ORM\Tools\ORMSetup;
 *     use Doctrine\ORM\EntityManager;
 *
 *     $paths = ['/path/to/entity/mapping/files'];
 *
 *     $config = ORMSetup::createAttributeMetadataConfig($paths);
 *     $connection = DriverManager::getConnection(['driver' => 'pdo_sqlite', 'memory' => true], $config);
 *     $entityManager = new EntityManager($connection, $config);
 *
 * For more information see
 * {@link http://docs.doctrine-project.org/projects/doctrine-orm/en/stable/reference/configuration.html}
 *
 * You should never attempt to inherit from the EntityManager: Inheritance
 * is not a valid extension point for the EntityManager. Instead you
 * should take a look at the {@see \Doctrine\ORM\Decorator\EntityManagerDecorator}
 * and wrap your entity manager in a decorator.
 *
 * @final
 */
class EntityManager implements EntityManagerInterface
{
    /**
     * The metadata factory, used to retrieve the ORM metadata of entity classes.
     */
    private ClassMetadataFactory $metadataFactory;

    /**
     * The UnitOfWork used to coordinate object-level transactions.
     */
    private UnitOfWork $unitOfWork;

    /**
     * The event manager that is the central point of the event system.
     */
    private EventManager $eventManager;

    /**
     * The proxy factory used to create dynamic proxies.
     */
    private ProxyFactory $proxyFactory;

    /**
     * The repository factory used to create dynamic repositories.
     */
    private RepositoryFactory $repositoryFactory;

    /**
     * The expression builder instance used to generate query expressions.
     */
    private Expr|null $expressionBuilder = null;

    /**
     * Whether the EntityManager is closed or not.
     */
    private bool $closed = false;

    /**
     * Collection of query filters.
     */
    private FilterCollection|null $filterCollection = null;

    /**
     * The second level cache regions API.
     */
    private Cache|null $cache = null;

    /**
     * Creates a new EntityManager that operates on the given database connection
     * and uses the given Configuration and EventManager implementations.
     *
     * @param Connection $conn The database connection used by the EntityManager.
     */
    public function __construct(
        private Connection $conn,
        private Configuration $config,
        EventManager|null $eventManager = null,
    ) {}

    public function getConnection(): Connection
    {}

    public function getMetadataFactory(): ClassMetadataFactory
    {}

    public function getExpressionBuilder(): Expr
    {}

    public function beginTransaction(): void
    {}

    public function getCache(): Cache|null
    {}

    public function wrapInTransaction(callable $func): mixed
    {}

    public function commit(): void
    {}

    public function rollback(): void
    {}

    /**
     * Returns the ORM metadata descriptor for a class.
     *
     * Internal note: Performance-sensitive method.
     *
     * {@inheritDoc}
     */
    public function getClassMetadata(string $className): Mapping\ClassMetadata
    {}

    public function createQuery(string $dql = ''): Query
    {}

    public function createNativeQuery(string $sql, ResultSetMapping $rsm): NativeQuery
    {}

    public function createQueryBuilder(): QueryBuilder
    {}

    /**
     * Flushes all changes to objects that have been queued up to now to the database.
     * This effectively synchronizes the in-memory state of managed objects with the
     * database.
     *
     * If an entity is explicitly passed to this method only this entity and
     * the cascade-persist semantics + scheduled inserts/removals are synchronized.
     *
     * @throws OptimisticLockException If a version check on an entity that
     * makes use of optimistic locking fails.
     * @throws ORMException
     */
    public function flush(): void
    {}

    /**
     * {@inheritDoc}
     */
    public function find($className, mixed $id, LockMode|int|null $lockMode = LockMode::NONE, int|null $lockVersion = null): object|null
    {}

    public function getReference(string $entityName, mixed $id): object|null
    {}

    /**
     * Clears the EntityManager. All entities that are currently managed
     * by this EntityManager become detached.
     */
    public function clear(): void
    {}

    public function close(): void
    {}

    /**
     * Tells the EntityManager to make an instance managed and persistent.
     *
     * The entity will be entered into the database at or before transaction
     * commit or as a result of the flush operation.
     *
     * NOTE: The persist operation always considers entities that are not yet known to
     * this EntityManager as NEW. Do not pass detached entities to the persist operation.
     *
     * @throws ORMInvalidArgumentException
     * @throws ORMException
     */
    public function persist(object $object): void
    {}

    /**
     * Removes an entity instance.
     *
     * A removed entity will be removed from the database at or before transaction commit
     * or as a result of the flush operation.
     *
     * @throws ORMInvalidArgumentException
     * @throws ORMException
     */
    public function remove(object $object): void
    {}

    public function refresh(object $object, LockMode|int|null $lockMode = LockMode::NONE): void
    {}

    /**
     * Detaches an entity from the EntityManager, causing a managed entity to
     * become detached.  Unflushed changes made to the entity if any
     * (including removal of the entity), will not be synchronized to the database.
     * Entities which previously referenced the detached entity will continue to
     * reference it.
     *
     * @throws ORMInvalidArgumentException
     */
    public function detach(object $object): void
    {}

    public function lock(object $entity, LockMode|int $lockMode, DateTimeInterface|int|null $lockVersion = null): void
    {}

    /**
     * Gets the repository for an entity class.
     *
     * @param class-string<T> $className The name of the entity.
     *
     * @return EntityRepository<T> The repository class.
     *
     * @template T of object
     */
    public function getRepository(string $className): EntityRepository
    {}

    /**
     * Determines whether an entity instance is managed in this EntityManager.
     *
     * @return bool TRUE if this EntityManager currently manages the given entity, FALSE otherwise.
     */
    public function contains(object $object): bool
    {}

    public function getEventManager(): EventManagerInterface
    {}

    public function getConfiguration(): Configuration
    {}

    /**
     * Throws an exception if the EntityManager is closed or currently not active.
     *
     * @throws EntityManagerClosed If the EntityManager is closed.
     */
    private function errorIfClosed(): void
    {}

    public function isOpen(): bool
    {}

    public function getUnitOfWork(): UnitOfWork
    {}

    public function newHydrator(string|int $hydrationMode): AbstractHydrator
    {}

    public function getProxyFactory(): ProxyFactory
    {}

    public function initializeObject(object $obj): void
    {}

    /**
     * {@inheritDoc}
     */
    public function isUninitializedObject($value): bool
    {}

    public function getFilters(): FilterCollection
    {}

    public function isFiltersStateClean(): bool
    {}

    public function hasFilters(): bool
    {}

    /**
     * @phpstan-param LockMode::* $lockMode
     *
     * @throws OptimisticLockException
     * @throws TransactionRequiredException
     */
    private function checkLockRequirements(LockMode|int $lockMode, ClassMetadata $class): void
    {}

    private function configureMetadataCache(): void
    {}

    private function createCustomHydrator(string $hydrationMode): AbstractHydrator
    {}
}
