<?php

namespace Bug3266;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @phpstan-template TKey of array-key
	 * @phpstan-template TValue
	 * @phpstan-param  array<TKey, TValue>  $iterator
	 * @phpstan-return array<TKey, TValue>
	 */
	public function iteratorToArray($iterator)
	{
		assertType('array<TKey of (int|string) (method Bug3266\Foo::iteratorToArray(), argument), TValue (method Bug3266\Foo::iteratorToArray(), argument)>', $iterator);
		$array = [];
		foreach ($iterator as $key => $value) {
			assertType('TKey of (int|string) (method Bug3266\Foo::iteratorToArray(), argument)', $key);
			assertType('TValue (method Bug3266\Foo::iteratorToArray(), argument)', $value);
			$array[$key] = $value;
			assertType('non-empty-array<TKey of (int|string) (method Bug3266\Foo::iteratorToArray(), argument), TValue (method Bug3266\Foo::iteratorToArray(), argument)>', $array);
		}

		assertType('array<TKey of (int|string) (method Bug3266\Foo::iteratorToArray(), argument), TValue (method Bug3266\Foo::iteratorToArray(), argument)>', $array);

		return $array;
	}

}
