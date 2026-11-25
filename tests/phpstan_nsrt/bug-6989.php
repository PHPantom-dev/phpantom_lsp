<?php

namespace Bug6989;

use function PHPStan\Testing\assertType;

class MyClass
{
	public const MY_KEY = 'key';

	/**
	 * @param array{static::MY_KEY: string} $items1
	 * @param array{self::MY_KEY: string} $items2
	 * @param array{MyClass::MY_KEY: string} $items3
	 *
	 * @return string
	 */
	public function myMethod(array $items1, array $items2, array $items3): array
	{
		assertType('array{key: string}', $items1);
		assertType('array{key: string}', $items2);
		assertType('array{key: string}', $items3);

		return $items1[static::MY_KEY];
	}
}

class ParentClass extends MyClass
{
	public const MY_KEY = 'different_key';

	/**
	 * @param array{static::MY_KEY: string} $items1
	 * @param array{self::MY_KEY: string} $items2
	 * @param array{MyClass::MY_KEY: string} $items3
	 * @param array{ParentClass::MY_KEY: string} $items4
	 * @param array{parent::MY_KEY: string} $items5
	 *
	 * @return string
	 */
	public function myMethod2(array $items1, array $items2, array $items3, array $items4, array $items5): array
	{
		assertType('array{different_key: string}', $items1);
		assertType('array{different_key: string}', $items2);
		assertType('array{key: string}', $items3);
		assertType('array{different_key: string}', $items4);
		assertType('array{key: string}', $items5);

		return $items1[static::MY_KEY];
	}
}
