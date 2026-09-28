<?php

namespace ClosurePassedToType;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @template T
	 * @template U
	 * @param array<T> $items
	 * @param callable(T): U $cb
	 * @return array<U>
	 */
	public function doFoo(array $items, callable $cb)
	{

	}

	public function doBar()
	{
		$a = [1, 2, 3];
		$b = $this->doFoo($a, function ($item) {
			assertType('1|2|3', $item);
			return $item;
		});
		assertType('array<1|2|3>', $b);
	}

	public function doBaz()
	{
		$a = [1, 2, 3];
		$b = $this->doFoo($a, fn ($item) => $item);
		assertType('array<1|2|3>', $b);
	}

}
