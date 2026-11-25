<?php

namespace GenericsEmptyArray;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @template TKey of array-key
	 * @template T
	 * @param array<T, TKey> $a
	 * @return array{TKey, T}
	 */
	public function doFoo(array $a = []): array
	{

	}

	public function doBar()
	{
		assertType('array{array-key, mixed}', $this->doFoo()); // PHPStan says `array{*NEVER*, *NEVER*}`; an omitted `= []` leaves the templates at their bounds, so a collection made empty still accepts items later
		assertType('array{*NEVER*, *NEVER*}', $this->doFoo([]));
	}

}

/**
 * @template TKey of array-key
 * @template T
 */
class ArrayCollection
{

	/**
	 * @param array<TKey, T> $items
	 */
	public function __construct(array $items = [])
	{

	}

}

class Bar
{

	public function doFoo()
	{
		assertType('GenericsEmptyArray\\ArrayCollection<array-key, mixed>', new ArrayCollection()); // PHPStan says `ArrayCollection<*NEVER*, *NEVER*>`; an omitted `= []` leaves the templates at their bounds, so a collection made empty still accepts items later
		assertType('GenericsEmptyArray\\ArrayCollection<*NEVER*, *NEVER*>', new ArrayCollection([]));
	}

}

/**
 * @template TKey of array-key
 * @template T
 */
class ArrayCollection2
{

	public function __construct(array $items = [])
	{

	}

}

class Baz
{

	public function doFoo()
	{
		assertType('GenericsEmptyArray\\ArrayCollection2<(int|string), mixed>', new ArrayCollection2());
		assertType('GenericsEmptyArray\\ArrayCollection2<(int|string), mixed>', new ArrayCollection2([]));
	}

}
