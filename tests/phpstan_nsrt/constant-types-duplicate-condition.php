<?php

namespace DuplicateConditionNeverError;

use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo(int $a, int $b)
	{
		$c = 0;

		if ($c === $a && $c === $b) {
			assertType('0', $a);
			assertType('0', $b);
			assertType('0', $c);
			return +1;
		}

		assertType('int', $a);

		assertType('int', $b);

		assertType('0', $c);

		if ($c === $a) {
			return -1;
		}


		assertType('int', $b);

		assertType('0', $c);

		if ($c === $b) {
			return +1;
		}



		assertType('0', $c);
	}

}
