<?php

declare(strict_types=1);

namespace Doctrine\DBAL;

use Closure;
use Doctrine\DBAL\Cache\ArrayResult;
use Doctrine\DBAL\Cache\CacheException;
use Doctrine\DBAL\Cache\Exception\NoResultDriverConfigured;
use Doctrine\DBAL\Cache\QueryCacheProfile;
use Doctrine\DBAL\Connection\StaticServerVersionProvider;
use Doctrine\DBAL\Driver\API\ExceptionConverter;
use Doctrine\DBAL\Driver\Connection as DriverConnection;
use Doctrine\DBAL\Driver\Exception as TheDriverException;
use Doctrine\DBAL\Driver\Statement as DriverStatement;
use Doctrine\DBAL\Exception\CommitFailedRollbackOnly;
use Doctrine\DBAL\Exception\ConnectionLost;
use Doctrine\DBAL\Exception\DeadlockException;
use Doctrine\DBAL\Exception\DriverException;
use Doctrine\DBAL\Exception\ForeignKeyConstraintViolationException;
use Doctrine\DBAL\Exception\NoActiveTransaction;
use Doctrine\DBAL\Exception\ParseError;
use Doctrine\DBAL\Exception\SavepointsNotSupported;
use Doctrine\DBAL\Exception\TransactionRolledBack;
use Doctrine\DBAL\Exception\UniqueConstraintViolationException;
use Doctrine\DBAL\Platforms\AbstractPlatform;
use Doctrine\DBAL\Query\Expression\ExpressionBuilder;
use Doctrine\DBAL\Query\QueryBuilder;
use Doctrine\DBAL\Schema\AbstractSchemaManager;
use Doctrine\DBAL\Schema\DefaultSchemaManagerFactory;
use Doctrine\DBAL\Schema\SchemaManagerFactory;
use Doctrine\DBAL\SQL\Parser;
use Doctrine\DBAL\Types\Type;
use Doctrine\Deprecations\Deprecation;
use InvalidArgumentException;
use SensitiveParameter;
use Throwable;
use Traversable;

use function array_key_exists;
use function array_merge;
use function count;
use function implode;
use function is_array;
use function is_int;
use function is_string;
use function key;
use function sprintf;

/**
 * A database abstraction-level connection that implements features like transaction isolation levels,
 * configuration, emulated transaction nesting, lazy connecting and more.
 *
 * @phpstan-import-type Params from DriverManager
 * @phpstan-type WrapperParameterType = string|Type|ParameterType|ArrayParameterType
 * @phpstan-type WrapperParameterTypeArray = array<
 *    int<0, max>,
 *    WrapperParameterType>|array<string, WrapperParameterType
 *  >
 * @phpstan-consistent-constructor
 */
class Connection implements ServerVersionProvider
{
    /**
     * The wrapped driver connection.
     */
    protected ?DriverConnection $_conn = null;

    protected Configuration $_config;

    /**
     * The current auto-commit mode of this connection.
     */
    private bool $autoCommit = true;

    /**
     * The transaction nesting level.
     */
    private int $transactionNestingLevel = 0;

    /**
     * The currently active transaction isolation level or NULL before it has been determined.
     */
    private ?TransactionIsolationLevel $transactionIsolationLevel = null;

    /**
     * The parameters used during creation of the Connection instance.
     *
     * @var array<string,mixed>
     * @phpstan-var Params
     */
    private array $params;

    /**
     * The database platform object used by the connection or NULL before it's initialized.
     */
    private ?AbstractPlatform $platform = null;

    private ?ExceptionConverter $exceptionConverter = null;
    private ?Parser $parser                         = null;

    /**
     * Flag that indicates whether the current transaction is marked for rollback only.
     */
    private bool $isRollbackOnly = false;

    private SchemaManagerFactory $schemaManagerFactory;

    /**
     * Initializes a new instance of the Connection class.
     *
     * @internal The connection can be only instantiated by the driver manager.
     *
     * @param array<string, mixed> $params The connection parameters.
     * @param Driver               $driver The driver to use.
     * @param Configuration|null   $config The configuration, optional.
     * @phpstan-param Params $params
     */
    public function __construct(
        #[SensitiveParameter]
        array $params,
        protected Driver $driver,
        ?Configuration $config = null,
    ) {}

    /**
     * Gets the parameters used during instantiation.
     *
     * @internal
     *
     * @return array<string,mixed>
     * @phpstan-return Params
     */
    public function getParams(): array
    {}

    /**
     * Gets the name of the currently selected database.
     *
     * @return ?non-empty-string The name of the database or NULL if a database is not selected.
     *                           The platforms which don't support the concept of a database (e.g. embedded databases)
     *                           must always return a string as an indicator of an implicitly selected database.
     *
     * @throws Exception
     */
    public function getDatabase(): ?string
    {}

    /**
     * Gets the DBAL driver instance.
     */
    public function getDriver(): Driver
    {}

    /**
     * Gets the Configuration used by the Connection.
     */
    public function getConfiguration(): Configuration
    {}

    /**
     * Gets the DatabasePlatform for the connection.
     *
     * @throws Exception
     */
    public function getDatabasePlatform(): AbstractPlatform
    {}

    /**
     * Creates an expression builder for the connection.
     */
    public function createExpressionBuilder(): ExpressionBuilder
    {}

    /**
     * Establishes the connection with the database and returns the underlying connection.
     *
     * @throws Exception
     */
    protected function connect(): DriverConnection
    {}

    /**
     * {@inheritDoc}
     *
     * @throws Exception
     */
    public function getServerVersion(): string
    {}

    /**
     * Returns the current auto-commit mode for this connection.
     *
     * @see    setAutoCommit
     *
     * @return bool True if auto-commit mode is currently enabled for this connection, false otherwise.
     */
    public function isAutoCommit(): bool
    {}

    /**
     * Sets auto-commit mode for this connection.
     *
     * If a connection is in auto-commit mode, then all its SQL statements will be executed and committed as individual
     * transactions. Otherwise, its SQL statements are grouped into transactions that are terminated by a call to either
     * the method commit or the method rollback. By default, new connections are in auto-commit mode.
     *
     * NOTE: If this method is called during a transaction and the auto-commit mode is changed, the transaction is
     * committed. If this method is called and the auto-commit mode is not changed, the call is a no-op.
     *
     * @see isAutoCommit
     *
     * @throws Exception
     */
    public function setAutoCommit(bool $autoCommit): void
    {}

    /**
     * Prepares and executes an SQL query and returns the first row of the result
     * as an associative array.
     *
     * @param list<mixed>|array<string, mixed> $params
     * @phpstan-param WrapperParameterTypeArray $types
     *
     * @return array<string, mixed>|false False is returned if no rows are found.
     *
     * @throws Exception
     */
    public function fetchAssociative(string $query, array $params = [], array $types = []): array|false
    {}

    /**
     * Prepares and executes an SQL query and returns the first row of the result
     * as a numerically indexed array.
     *
     * @param list<mixed>|array<string, mixed> $params
     * @phpstan-param WrapperParameterTypeArray $types
     *
     * @return list<mixed>|false False is returned if no rows are found.
     *
     * @throws Exception
     */
    public function fetchNumeric(string $query, array $params = [], array $types = []): array|false
    {}

    /**
     * Prepares and executes an SQL query and returns the value of a single column
     * of the first row of the result.
     *
     * @param list<mixed>|array<string, mixed> $params
     * @phpstan-param WrapperParameterTypeArray $types
     *
     * @return mixed|false False is returned if no rows are found.
     *
     * @throws Exception
     */
    public function fetchOne(string $query, array $params = [], array $types = []): mixed
    {}

    /**
     * Whether an actual connection to the database is established.
     *
     * @phpstan-assert-if-true !null $this->_conn
     */
    public function isConnected(): bool
    {}

    /**
     * Checks whether a transaction is currently active.
     *
     * @return bool TRUE if a transaction is currently active, FALSE otherwise.
     */
    public function isTransactionActive(): bool
    {}

    /**
     * Adds condition based on the criteria to the query components
     *
     * @param array<string, mixed> $criteria Map of key columns to their values
     *
     * @return array{list<string>, list<mixed>, list<string>}
     */
    private function getCriteriaCondition(array $criteria): array
    {}

    /**
     * Executes an SQL DELETE statement on a table.
     *
     * Table expression and columns are not escaped and are not safe for user-input.
     *
     * @param array<string, mixed>                                                                  $criteria
     * @param array<int<0,max>, string|ParameterType|Type>|array<string, string|ParameterType|Type> $types
     *
     * @return int|numeric-string The number of affected rows.
     *
     * @throws Exception
     */
    public function delete(string $table, array $criteria = [], array $types = []): int|string
    {}

    /**
     * Closes the connection.
     */
    public function close(): void
    {}

    /**
     * Sets the transaction isolation level.
     *
     * @param TransactionIsolationLevel $level The level to set.
     *
     * @throws Exception
     */
    public function setTransactionIsolation(TransactionIsolationLevel $level): void
    {}

    /**
     * Gets the currently active transaction isolation level.
     *
     * @return TransactionIsolationLevel The current transaction isolation level.
     *
     * @throws Exception
     */
    public function getTransactionIsolation(): TransactionIsolationLevel
    {}

    /**
     * Executes an SQL UPDATE statement on a table.
     *
     * Table expression and columns are not escaped and are not safe for user-input.
     *
     * @param array<string, mixed>                                                                  $data
     * @param array<string, mixed>                                                                  $criteria
     * @param array<int<0,max>, string|ParameterType|Type>|array<string, string|ParameterType|Type> $types
     *
     * @return int|numeric-string The number of affected rows.
     *
     * @throws Exception
     */
    public function update(string $table, array $data, array $criteria = [], array $types = []): int|string
    {}

    /**
     * Inserts a table row with specified data.
     *
     * Table expression and columns are not escaped and are not safe for user-input.
     *
     * @param array<string, mixed>                                                                  $data
     * @param array<int<0,max>, string|ParameterType|Type>|array<string, string|ParameterType|Type> $types
     *
     * @return int|numeric-string The number of affected rows.
     *
     * @throws Exception
     */
    public function insert(string $table, array $data, array $types = []): int|string
    {}

    /**
     * Extract ordered type list from an ordered column list and type map.
     *
     * @param array<int, string>                                                             $columns
     * @param array<int, string|ParameterType|Type>|array<string, string|ParameterType|Type> $types
     *
     * @return array<int<0, max>, string|ParameterType|Type>
     */
    private function extractTypeValues(array $columns, array $types): array
    {}

    /**
     * Quotes a string so it can be safely used as a table or column name, even if
     * it is a reserved name.
     *
     * Delimiting style depends on the underlying database platform that is being used.
     *
     * NOTE: Just because you CAN use quoted identifiers does not mean
     * you SHOULD use them. In general, they end up causing way more
     * problems than they solve.
     *
     * @deprecated Use {@link quoteSingleIdentifier()} individually for each part of a qualified name instead.
     *
     * @param string $identifier The identifier to be quoted.
     *
     * @return string The quoted identifier.
     *
     * @throws Exception
     */
    public function quoteIdentifier(string $identifier): string
    {}

    /**
     * Quotes a string so that it can be safely used as an identifier in SQL.
     *
     * @throws Exception
     */
    public function quoteSingleIdentifier(string $identifier): string
    {}

    /**
     * The usage of this method is discouraged. Use prepared statements
     * or {@see AbstractPlatform::quoteStringLiteral()} instead.
     *
     * @throws Exception
     */
    public function quote(string $value): string
    {}

    /**
     * Prepares and executes an SQL query and returns the result as an array of numeric arrays.
     *
     * @param list<mixed>|array<string, mixed> $params
     * @phpstan-param WrapperParameterTypeArray $types
     *
     * @return list<list<mixed>>
     *
     * @throws Exception
     */
    public function fetchAllNumeric(string $query, array $params = [], array $types = []): array
    {}

    /**
     * Prepares and executes an SQL query and returns the result as an array of associative arrays.
     *
     * @param list<mixed>|array<string, mixed> $params
     * @phpstan-param WrapperParameterTypeArray $types
     *
     * @return list<array<string,mixed>>
     *
     * @throws Exception
     */
    public function fetchAllAssociative(string $query, array $params = [], array $types = []): array
    {}

    /**
     * Prepares and executes an SQL query and returns the result as an associative array with the keys
     * mapped to the first column and the values mapped to the second column.
     *
     * @param list<mixed>|array<string, mixed> $params
     * @phpstan-param WrapperParameterTypeArray $types
     *
     * @return array<mixed,mixed>
     *
     * @throws Exception
     */
    public function fetchAllKeyValue(string $query, array $params = [], array $types = []): array
    {}

    /**
     * Prepares and executes an SQL query and returns the result as an associative array with the keys mapped
     * to the first column and the values being an associative array representing the rest of the columns
     * and their values.
     *
     * @param list<mixed>|array<string, mixed> $params
     * @phpstan-param WrapperParameterTypeArray $types
     *
     * @return array<mixed,array<string,mixed>>
     *
     * @throws Exception
     */
    public function fetchAllAssociativeIndexed(string $query, array $params = [], array $types = []): array
    {}

    /**
     * Prepares and executes an SQL query and returns the result as an array of the first column values.
     *
     * @param list<mixed>|array<string, mixed> $params
     * @phpstan-param WrapperParameterTypeArray $types
     *
     * @return list<mixed>
     *
     * @throws Exception
     */
    public function fetchFirstColumn(string $query, array $params = [], array $types = []): array
    {}

    /**
     * Prepares and executes an SQL query and returns the result as an iterator over rows represented as numeric arrays.
     *
     * @param list<mixed>|array<string, mixed> $params
     * @phpstan-param WrapperParameterTypeArray $types
     *
     * @return Traversable<int,list<mixed>>
     *
     * @throws Exception
     */
    public function iterateNumeric(string $query, array $params = [], array $types = []): Traversable
    {}

    /**
     * Prepares and executes an SQL query and returns the result as an iterator over rows represented
     * as associative arrays.
     *
     * @param list<mixed>|array<string, mixed> $params
     * @phpstan-param WrapperParameterTypeArray $types
     *
     * @return Traversable<int,array<string,mixed>>
     *
     * @throws Exception
     */
    public function iterateAssociative(string $query, array $params = [], array $types = []): Traversable
    {}

    /**
     * Prepares and executes an SQL query and returns the result as an iterator with the keys
     * mapped to the first column and the values mapped to the second column.
     *
     * @param list<mixed>|array<string, mixed> $params
     * @phpstan-param WrapperParameterTypeArray $types
     *
     * @return Traversable<mixed,mixed>
     *
     * @throws Exception
     */
    public function iterateKeyValue(string $query, array $params = [], array $types = []): Traversable
    {}

    /**
     * Prepares and executes an SQL query and returns the result as an iterator with the keys mapped
     * to the first column and the values being an associative array representing the rest of the columns
     * and their values.
     *
     * @param list<mixed>|array<string, mixed> $params
     * @phpstan-param WrapperParameterTypeArray $types
     *
     * @return Traversable<mixed,array<string,mixed>>
     *
     * @throws Exception
     */
    public function iterateAssociativeIndexed(string $query, array $params = [], array $types = []): Traversable
    {}

    /**
     * Prepares and executes an SQL query and returns the result as an iterator over the first column values.
     *
     * @param list<mixed>|array<string, mixed> $params
     * @phpstan-param WrapperParameterTypeArray $types
     *
     * @return Traversable<int,mixed>
     *
     * @throws Exception
     */
    public function iterateColumn(string $query, array $params = [], array $types = []): Traversable
    {}

    /**
     * Prepares an SQL statement.
     *
     * @param string $sql The SQL statement to prepare.
     *
     * @throws Exception
     */
    public function prepare(string $sql): Statement
    {}

    /**
     * Executes an, optionally parameterized, SQL query.
     *
     * If the query is parametrized, a prepared statement is used.
     *
     * @param list<mixed>|array<string, mixed> $params
     * @phpstan-param WrapperParameterTypeArray $types
     *
     * @throws Exception
     */
    public function executeQuery(
        string $sql,
        array $params = [],
        array $types = [],
        ?QueryCacheProfile $qcp = null,
    ): Result {}

    /**
     * Executes a caching query.
     *
     * @param list<mixed>|array<string, mixed> $params
     * @phpstan-param WrapperParameterTypeArray $types
     *
     * @throws CacheException
     * @throws Exception
     */
    public function executeCacheQuery(string $sql, array $params, array $types, QueryCacheProfile $qcp): Result
    {}

    /**
     * Executes an SQL statement with the given parameters and returns the number of affected rows.
     *
     * Could be used for:
     *  - DML statements: INSERT, UPDATE, DELETE, etc.
     *  - DDL statements: CREATE, DROP, ALTER, etc.
     *  - DCL statements: GRANT, REVOKE, etc.
     *  - Session control statements: ALTER SESSION, SET, DECLARE, etc.
     *  - Other statements that don't yield a row set.
     *
     * This method supports PDO binding types as well as DBAL mapping types.
     *
     * @param list<mixed>|array<string, mixed> $params
     * @phpstan-param WrapperParameterTypeArray $types
     *
     * @return int|numeric-string
     *
     * @throws Exception
     */
    public function executeStatement(string $sql, array $params = [], array $types = []): int|string
    {}

    /**
     * Returns the current transaction nesting level.
     *
     * @return int The nesting level. A value of 0 means there's no active transaction.
     */
    public function getTransactionNestingLevel(): int
    {}

    /**
     * Returns the ID of the last inserted row.
     *
     * If the underlying driver does not support identity columns, an exception is thrown.
     *
     * @throws Exception
     */
    public function lastInsertId(): int|string
    {}

    /**
     * Executes a function in a transaction.
     *
     * The function gets passed this Connection instance as an (optional) parameter.
     *
     * If an exception occurs during execution of the function or transaction commit,
     * the transaction is rolled back and the exception re-thrown.
     *
     * @param Closure(self):T $func The function to execute transactionally.
     *
     * @return T The value returned by $func
     *
     * @throws Throwable
     *
     * @template T
     */
    public function transactional(Closure $func): mixed
    {}

    /**
     * Sets if nested transactions should use savepoints.
     *
     * @deprecated No replacement planned
     *
     * @throws Exception
     */
    public function setNestTransactionsWithSavepoints(bool $nestTransactionsWithSavepoints): void
    {}

    /**
     * Gets if nested transactions should use savepoints.
     *
     * @deprecated No replacement planned
     */
    public function getNestTransactionsWithSavepoints(): bool
    {}

    /**
     * Returns the savepoint name to use for nested transactions.
     */
    protected function _getNestedTransactionSavePointName(): string
    {}

    /** @throws Exception */
    public function beginTransaction(): void
    {}

    /** @throws Exception */
    public function commit(): void
    {}

    /** @throws Exception */
    private function updateTransactionStateAfterCommit(): void
    {}

    /**
     * Commits all current nesting transactions.
     *
     * @throws Exception
     */
    private function commitAll(): void
    {}

    /** @throws Exception */
    public function rollBack(): void
    {}

    /**
     * Creates a new savepoint.
     *
     * @param string $savepoint The name of the savepoint to create.
     *
     * @throws Exception
     */
    public function createSavepoint(string $savepoint): void
    {}

    /**
     * Releases the given savepoint.
     *
     * @param string $savepoint The name of the savepoint to release.
     *
     * @throws Exception
     */
    public function releaseSavepoint(string $savepoint): void
    {}

    /**
     * Rolls back to the given savepoint.
     *
     * @param string $savepoint The name of the savepoint to rollback to.
     *
     * @throws Exception
     */
    public function rollbackSavepoint(string $savepoint): void
    {}

    /**
     * Provides access to the native database connection.
     *
     * @return resource|object
     *
     * @throws Exception
     */
    public function getNativeConnection()
    {}

    /**
     * Creates a SchemaManager that can be used to inspect or change the
     * database schema through the connection.
     *
     * @throws Exception
     */
    public function createSchemaManager(): AbstractSchemaManager
    {}

    /**
     * Marks the current transaction so that the only possible
     * outcome for the transaction to be rolled back.
     *
     * @throws ConnectionException If no transaction is active.
     */
    public function setRollbackOnly(): void
    {}

    /**
     * Checks whether the current transaction is marked for rollback only.
     *
     * @throws ConnectionException If no transaction is active.
     */
    public function isRollbackOnly(): bool
    {}

    /**
     * Converts a given value to its database representation according to the conversion
     * rules of a specific DBAL mapping type.
     *
     * @param mixed  $value The value to convert.
     * @param string $type  The name of the DBAL mapping type.
     *
     * @return mixed The converted value.
     *
     * @throws Exception
     */
    public function convertToDatabaseValue(mixed $value, string $type): mixed
    {}

    /**
     * Converts a given value to its PHP representation according to the conversion
     * rules of a specific DBAL mapping type.
     *
     * @param mixed  $value The value to convert.
     * @param string $type  The name of the DBAL mapping type.
     *
     * @return mixed The converted type.
     *
     * @throws Exception
     */
    public function convertToPHPValue(mixed $value, string $type): mixed
    {}

    /**
     * Binds a set of parameters, some or all of which are typed with a PDO binding type
     * or DBAL mapping type, to a given statement.
     *
     * @param list<mixed>|array<string, mixed>                                               $params
     * @param array<int, string|ParameterType|Type>|array<string, string|ParameterType|Type> $types
     *
     * @throws Exception
     */
    private function bindParameters(DriverStatement $stmt, array $params, array $types): void
    {}

    /**
     * Gets the binding type of a given type.
     *
     * @param mixed                     $value The value to bind.
     * @param string|ParameterType|Type $type  The type to bind.
     *
     * @return array{mixed, ParameterType} [0] => the (escaped) value, [1] => the binding type.
     *
     * @throws Exception
     */
    private function getBindingInfo(mixed $value, string|ParameterType|Type $type): array
    {}

    /**
     * Creates a new instance of a SQL query builder.
     */
    public function createQueryBuilder(): QueryBuilder
    {}

    /**
     * @internal
     *
     * @param list<mixed>|array<string,mixed> $params
     * @phpstan-param WrapperParameterTypeArray $types
     */
    final public function convertExceptionDuringQuery(
        Driver\Exception $e,
        string $sql,
        array $params = [],
        array $types = [],
    ): DriverException {}

    /** @internal */
    final public function convertException(Driver\Exception $e): DriverException
    {}

    /**
     * @param list<mixed>|array<string, mixed> $params
     * @phpstan-param WrapperParameterTypeArray $types
     *
     * @return array{
     *     string,
     *     list<mixed>|array<string, mixed>,
     *     array<int<0, max>, string|ParameterType|Type>|array<string, string|ParameterType|Type>
     * }
     *
     * @throws Exception
     */
    private function expandArrayParameters(string $sql, array $params, array $types): array
    {}

    private function handleDriverException(
        Driver\Exception $driverException,
        ?Query $query,
    ): DriverException {}
}
