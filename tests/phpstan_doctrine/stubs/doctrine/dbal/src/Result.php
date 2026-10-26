<?php

declare(strict_types=1);

namespace Doctrine\DBAL;

use Doctrine\DBAL\Driver\Exception as DriverException;
use Doctrine\DBAL\Driver\Result as DriverResult;
use Doctrine\DBAL\Exception\NoKeyValue;
use LogicException;
use Traversable;

use function array_shift;
use function assert;
use function count;
use function get_debug_type;
use function method_exists;
use function sprintf;

class Result
{
    /** @internal The result can be only instantiated by {@see Connection} or {@see Statement}. */
    public function __construct(private readonly DriverResult $result, private readonly Connection $connection)
    {}

    /**
     * Returns the next row of the result as a numeric array or FALSE if there are no more rows.
     *
     * @return list<mixed>|false
     *
     * @throws Exception
     */
    public function fetchNumeric(): array|false
    {}

    /**
     * Returns the next row of the result as an associative array or FALSE if there are no more rows.
     *
     * @return array<string,mixed>|false
     *
     * @throws Exception
     */
    public function fetchAssociative(): array|false
    {}

    /**
     * Returns the first value of the next row of the result or FALSE if there are no more rows.
     *
     * @throws Exception
     */
    public function fetchOne(): mixed
    {}

    /**
     * Returns an array containing all of the result rows represented as numeric arrays.
     *
     * @return list<list<mixed>>
     *
     * @throws Exception
     */
    public function fetchAllNumeric(): array
    {}

    /**
     * Returns an array containing all of the result rows represented as associative arrays.
     *
     * @return list<array<string,mixed>>
     *
     * @throws Exception
     */
    public function fetchAllAssociative(): array
    {}

    /**
     * Returns an array containing the values of the first column of the result.
     *
     * @return array<mixed,mixed>
     *
     * @throws Exception
     */
    public function fetchAllKeyValue(): array
    {}

    /**
     * Returns an associative array with the keys mapped to the first column and the values being
     * an associative array representing the rest of the columns and their values.
     *
     * @return array<mixed,array<string,mixed>>
     *
     * @throws Exception
     */
    public function fetchAllAssociativeIndexed(): array
    {}

    /**
     * @return list<mixed>
     *
     * @throws Exception
     */
    public function fetchFirstColumn(): array
    {}

    /**
     * @return Traversable<int,list<mixed>>
     *
     * @throws Exception
     */
    public function iterateNumeric(): Traversable
    {}

    /**
     * @return Traversable<int,array<string,mixed>>
     *
     * @throws Exception
     */
    public function iterateAssociative(): Traversable
    {}

    /**
     * @return Traversable<mixed, mixed>
     *
     * @throws Exception
     */
    public function iterateKeyValue(): Traversable
    {}

    /**
     * Returns an iterator over the result set with the keys mapped to the first column and the values being
     * an associative array representing the rest of the columns and their values.
     *
     * @return Traversable<mixed,array<string,mixed>>
     *
     * @throws Exception
     */
    public function iterateAssociativeIndexed(): Traversable
    {}

    /**
     * @return Traversable<int,mixed>
     *
     * @throws Exception
     */
    public function iterateColumn(): Traversable
    {}

    /**
     * Returns the number of rows affected by the DELETE, INSERT, or UPDATE statement that produced the result.
     *
     * If the statement executed a SELECT query or a similar platform-specific SQL (e.g. DESCRIBE, SHOW, etc.),
     * some database drivers may return the number of rows returned by that query. However, this behaviour
     * is not guaranteed for all drivers and should not be relied on in portable applications.
     *
     * If the number of rows exceeds {@see PHP_INT_MAX}, it might be returned as string if the driver supports it.
     *
     * @return int|numeric-string
     *
     * @throws Exception
     */
    public function rowCount(): int|string
    {}

    /** @throws Exception */
    public function columnCount(): int
    {}

    /**
     * Returns the name of the column in the result set for the given 0-based index.
     *
     * If the index is not a valid column index ({@see columnCount}), an exception will be thrown.
     *
     * @throws Exception
     */
    public function getColumnName(int $index): string
    {}

    public function free(): void
    {}

    /** @throws Exception */
    private function ensureHasKeyValue(): void
    {}
}
