<?php // lint >= 8.0

namespace Bug10473;

use ArrayAccess;
use function PHPStan\Testing\assertType;

/**
 * @template TRow of array<string, mixed>
 */
class Rows
{

	/**
	 * @param list<TRow> $rowsData
	 */
	public function __construct(private array $rowsData)
	{}

	/**
	 * @return Row<TRow>|NULL
	 */
	public function getByIndex(int $index): ?Row
	{
		return isset($this->rowsData[$index])
			? new Row($this->rowsData[$index])
			: NULL;
	}
}

/**
 * @template TRow of array<string, mixed>
 * @implements ArrayAccess<key-of<TRow>, value-of<TRow>>
 */
class Row implements ArrayAccess
{

	/**
	 * @param TRow $data
	 */
	public function __construct(private array $data)
	{}

	/**
	 * @param key-of<TRow> $key
	 */
	public function offsetExists($key): bool
	{
		return isset($this->data[$key]);
	}

	/**
	 * @template TKey of key-of<TRow>
	 * @param TKey $key
	 * @return TRow[TKey]
	 */
	public function offsetGet($key): mixed
	{
		return $this->data[$key];
	}

	public function offsetSet($key, mixed $value): void
	{
		$this->data[$key] = $value;
	}

	public function offsetUnset($key): void
	{
		unset($this->data[$key]);
	}

	/**
	 * @return TRow
	 */
	public function toArray(): array
	{
		return $this->data;
	}

}

class Foo
{

	/** @param Rows<array{foo: int<0, max>}> $rows */
	public function doFoo(Rows $rows): void
	{
		assertType('Bug10473\Rows<array{foo: int<0, max>}>', $rows);

		$row = $rows->getByIndex(0);

		if ($row !== NULL) {
			assertType('Bug10473\Row<array{foo: int<0, max>}>', $row);
			$fooFromRow = $row['foo'];

			assertType('array{foo: int<0, max>}', $row->toArray());
		}

	}

}
