<?php

namespace Bug4117Types;

use ArrayIterator;
use IteratorAggregate;
use function PHPStan\Testing\assertType;

/**
 * @refactor Refactor utils into base library
 * @template T of mixed
 * @implements IteratorAggregate<int, T>
 */
class GenericList implements IteratorAggregate
{
	/** @var array<int, T> */
	protected $items = [];

	/**
	 * @return ArrayIterator<int, T>
	 */
	public function getIterator(): ArrayIterator
	{
		return new ArrayIterator($this->items);
	}

	/**
	 * @return ?T
	 */
	public function broken(int $key)
	{
		$item = $this->items[$key] ?? null;
		assertType('T of mixed~null (class Bug4117Types\GenericList, argument)|null', $item);
		if ($item) {
			assertType("T of mixed~(0|0.0|''|'0'|array{}|false|null) (class Bug4117Types\GenericList, argument)", $item);
		} else {
		}

		assertType('T of mixed~null (class Bug4117Types\GenericList, argument)|null', $item);

		return $item;
	}

	/**
	 * @return ?T
	 */
	public function works(int $key)
	{
		$item = $this->items[$key] ?? null;
		assertType('T of mixed~null (class Bug4117Types\GenericList, argument)|null', $item);

		return $item;
	}
}
