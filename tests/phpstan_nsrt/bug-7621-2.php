<?php declare(strict_types = 1);

namespace Bug7621Two;

use function PHPStan\Testing\assertType;

class Foo
{
	private const FOO = [ 'foo' => ['foo', 'bar'] ];

	public function foo(): void
	{
		assertType("array{'foo', 'bar'}", self::FOO['foo']);
		$keys = [0, 1, 2];
		foreach ($keys as $key) {
			if (array_key_exists($key, self::FOO['foo'])) {
				assertType("array{'foo', 'bar'}", self::FOO['foo']);
			} else {
				assertType("array{'foo', 'bar'}", self::FOO['foo']);
			}
		}
	}
}
