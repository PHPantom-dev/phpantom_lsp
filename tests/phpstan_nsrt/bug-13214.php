<?php

namespace Bug13214;

use function PHPStan\Testing\assertType;
use stdClass;

class HelloWorld
{
	/**
	 * @param ArrayAccess<int, ?object> $array
	 */
	public function sayHello(ArrayAccess $array): void
	{
		$child = new stdClass();

		assert($array[1] === null);

		assertType('null', $array[1]);

		$array[1] = $child;

		assertType(stdClass::class, $array[1]);
	}

	/**
	 * @param array<int, ?object> $array
	 */
	public function sayHelloArray(array $array): void
	{
		$child = new stdClass();

		assert(($array[1] ?? null) === null);

		assertType('object|null', $array[1]);

		$array[1] = $child;

		assertType(stdClass::class, $array[1]);
	}
}
