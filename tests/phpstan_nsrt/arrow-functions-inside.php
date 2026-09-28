<?php

namespace ArrowFunctionsInside;

use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo(int $i)
	{
		fn(string $s) => [
			assertType('int', $i),
			assertType('string', $s),
			assertType('*ERROR*', $t),
		];
	}

}
