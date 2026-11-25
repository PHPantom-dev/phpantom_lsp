<?php

namespace FiniteTypes;

use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo(bool $a, bool $b): void
	{
		$array = [$a, $b];
		if ($array === [true, false]) {
			assertType('array{true, false}', $array);
			return;
		}

		if ($array === [false, false]) {
			assertType('array{false, false}', $array);
			return;
		}

		if ($array === [true, true]) {
			assertType('array{true, true}', $array);
			return;
		}

	}

}
