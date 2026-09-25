<?php

namespace EmptyArrayShape;

use function PHPStan\Testing\assertType;

class Foo
{

	/** @param array{} $array */
	public function doFoo(array $array): void
	{
		assertType('array{}', $array);
	}

}
