<?php

namespace InferArrayKey;

use function PHPStan\Testing\assertType;

/**
 * @implements \IteratorAggregate<int, \stdClass>
 */
class Foo implements \IteratorAggregate
{

	/** @var \stdClass[] */
	private $items;

	#[\ReturnTypeWillChange]
	public function getIterator()
	{
		$it = new \ArrayIterator($this->items);
		// PHPantom follows the stub's `TKey|null`: key() returns null once the iterator is past its end.
		assertType('int|string|null', $it->key());

		return $it;
	}

}

/**
 * @implements \IteratorAggregate<int, \stdClass>
 */
class Bar implements \IteratorAggregate
{

	/** @var array<int, \stdClass> */
	private $items;

	#[\ReturnTypeWillChange]
	public function getIterator()
	{
		$it = new \ArrayIterator($this->items);
		// PHPantom follows the stub's `TKey|null`, as above.
		assertType('int|null', $it->key());

		return $it;
	}

}

/**
 * @implements \IteratorAggregate<string, \stdClass>
 */
class Baz implements \IteratorAggregate
{

	/** @var array<string, \stdClass> */
	private $items;

	#[\ReturnTypeWillChange]
	public function getIterator()
	{
		$it = new \ArrayIterator($this->items);
		// PHPantom follows the stub's `TKey|null`, as above.
		assertType('string|null', $it->key());

		return $it;
	}

}

/**
 * @implements \IteratorAggregate<int, \stdClass>
 */
class Lorem implements \IteratorAggregate
{

	/** @var array<\stdClass> */
	private $items;

	#[\ReturnTypeWillChange]
	public function getIterator()
	{
		$it = new \ArrayIterator($this->items);
		// PHPantom follows the stub's `TKey|null`, as above.
		assertType('int|string|null', $it->key());

		return $it;
	}

}

/**
 * @implements \IteratorAggregate<int|string, \stdClass>
 */
class Ipsum implements \IteratorAggregate
{

	/** @var array<int|string, \stdClass> */
	private $items;

	#[\ReturnTypeWillChange]
	public function getIterator()
	{
		$it = new \ArrayIterator($this->items);
		// PHPantom follows the stub's `TKey|null`, as above.
		assertType('int|string|null', $it->key());

		return $it;
	}

}
