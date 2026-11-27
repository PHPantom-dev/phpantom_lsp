<?php

namespace RangeNumericString;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param numeric-string $a
	 * @param numeric-string $b
	 */
	public function doFoo(
		string $a,
		string $b
	): void
	{
		// PHPantom is right: single-digit numeric strings count through characters, so `range('1', '3')` is `['1', '2', '3']`.
		assertType('non-empty-list<int|float|string>', range($a, $b));
	}

}
