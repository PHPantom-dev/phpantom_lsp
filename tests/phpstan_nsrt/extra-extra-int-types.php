<?php

namespace ExtraExtraIntTypes;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param non-positive-int $nonPositiveInt
	 * @param non-negative-int $nonNegativeInt
	 */
	public function doFoo(
		int $nonPositiveInt,
		int $nonNegativeInt,
	): void
	{
		assertType('int<min, 0>', $nonPositiveInt);
		assertType('int<0, max>', $nonNegativeInt);
	}

}
