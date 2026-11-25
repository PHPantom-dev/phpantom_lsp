<?php declare(strict_types = 1);

namespace Bug7621Three;

use function PHPStan\Testing\assertType;

class Bar
{
	private const FOO = [ 'foo' => ['foo', 'bar'] ];


	/** @param 'foo'|'bar' $key */
	public function foo(string $key): void
	{
		if (!array_key_exists($key, self::FOO)) {
			assertType("array{foo: array{'foo', 'bar'}}", self::FOO);
			assertType("array{'foo', 'bar'}", self::FOO['foo']);
		}
	}
}
