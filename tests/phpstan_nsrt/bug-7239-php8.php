<?php // lint >= 8.0

declare(strict_types = 1);

namespace Bug7239php8;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	/**
	 * @param string[] $strings
	 */
	public function sayHello(array $arr, $strings): void
	{
		assertType('*ERROR*', max([]));
		assertType('*ERROR*', min([]));

		if (count($arr) > 0) {
			assertType('mixed', max($arr));
			assertType('mixed', min($arr));
		} else {
			assertType('*ERROR*', max($arr));
			assertType('*ERROR*', min($arr));
		}

		assertType('array', max([], $arr));
		assertType('array', min([], $arr));

		if (count($strings) > 0) {
			assertType('string', max($strings));
			assertType('string', min($strings));
		} else {
			assertType('*ERROR*', max($strings));
			assertType('*ERROR*', min($strings));
		}
	}
}
