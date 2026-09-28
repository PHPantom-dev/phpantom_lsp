<?php

namespace Bug5372;

use function PHPStan\Testing\assertType;

/**
 * @template TKey of array-key
 * @template T
 */
class Collection
{

	/** @var array<TKey, T> */
	private $values;

	/**
	 * @param array<TKey, T> $values
	 */
	public function __construct(array $values)
	{
		$this->values = $values;
	}

	/**
	 * @template V
	 *
	 * @param callable(T): V $callback
	 *
	 * @return self<TKey, V>
	 */
	public function map(callable $callback): self {
		return new self(array_map($callback, $this->values));
	}

	/**
	 * @template V of string
	 *
	 * @param callable(T): V $callback
	 *
	 * @return self<TKey, V>
	 */
	public function map2(callable $callback): self {
		return new self(array_map($callback, $this->values));
	}
}

class Foo
{

	/** @param Collection<int, string> $list */
	function takesStrings(Collection $list): void {
		echo serialize($list);
	}

	/** @param class-string $classString */
	public function doFoo(string $classString)
	{
		$col = new Collection(['foo', 'bar']);
		assertType('Bug5372\Collection<int, string>', $col);

		$newCol = $col->map(static fn(string $var): string => $var . 'bar');
		$this->takesStrings($newCol);

		$newCol = $col->map(static fn(string $var): string => $classString);
		assertType('Bug5372\Collection<int, class-string>', $newCol);
		$this->takesStrings($newCol);

		$newCol = $col->map2(static fn(string $var): string => $classString);
		assertType('Bug5372\Collection<int, class-string>', $newCol);
		$this->takesStrings($newCol);
	}

	/** @param literal-string $literalString */
	public function doBar(string $literalString)
	{
		$col = new Collection(['foo', 'bar']);
		$newCol = $col->map(static fn(string $var): string => $literalString);
		$this->takesStrings($newCol);

		$newCol = $col->map2(static fn(string $var): string => $literalString);
		$this->takesStrings($newCol);
	}

}
