<?php

namespace NullableClosureParameter;

use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo()
	{
		$a = function (string $test = null) {
			assertType('string|null', $test);
			return $test;
		};
		assertType('string|null', $a());

		$b = fn (string $test = null) => $test;
		assertType('string|null', $b());

		fn (string $test = null): string => assertType('string|null', $test);
	}

}
