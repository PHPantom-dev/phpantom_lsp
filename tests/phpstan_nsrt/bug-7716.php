<?php declare(strict_types = 1);

namespace Bug7716;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	/**
	 * @param array{foo?: int, bar?: int} $array
	 */
	public function sayHello(array $array): int
	{
		$hasFoo = isset($array['foo']) && $array['foo'] > 1;
		$hasBar = isset($array['bar']) && $array['bar'] > 1;

		if ($hasFoo) {
			assertType('array{foo: int<2, max>, bar?: int}', $array); // PHPStan keeps the declared shape; PHPantom is more precise here
			assertType('int<2, max>', $array['foo']);
			return $array['foo'];
		}

		if ($hasBar) {
			assertType('int<2, max>', $array['bar']);
			return $array['bar'];
		}

		return 0;
	}

	/**
	 * @param array{foo?: int, bar?: int} $array
	 */
	public function sayHello2(array $array): int
	{
		$hasBar = isset($array['bar']) && $array['bar'] > 1;
		$hasFoo = isset($array['foo']) && $array['foo'] > 1;

		if ($hasFoo) {
			assertType('int<2, max>', $array['foo']);
			return $array['foo'];
		}

		if ($hasBar) {
			assertType('array{foo?: int, bar: int<2, max>}', $array); // PHPStan keeps the declared shape; PHPantom is more precise here
			assertType('int<2, max>', $array['bar']);
			return $array['bar'];
		}

		return 0;
	}
}
