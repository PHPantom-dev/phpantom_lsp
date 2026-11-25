<?php

namespace Bug2539;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param array<int> $array
	 * @param non-empty-array<int> $nonEmptyArray
	 */
	public function doFoo(
		array $array,
		array $nonEmptyArray
	): void
	{
		assertType('int|false', current($array));
		assertType('int', current($nonEmptyArray));

		assertType('false', current([]));
		assertType('1|2|3', current([1, 2, 3]));

		$a = [];
		if (rand(0, 1)) {
			$a[] = 1;
		}

		assertType('1|false', current($a));
	}

}
