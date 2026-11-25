<?php

namespace Bug5458;

use function PHPStan\Testing\assertType;

class Foo {
	public const A = [
		'a',
		'b',
		'c',
	];

	public const B = [
		...self::A,
	];

	public function doFoo(): void
	{
		assertType("array{'a', 'b', 'c'}", self::A);
		assertType("array{'a', 'b', 'c'}", self::B);
	}
}
