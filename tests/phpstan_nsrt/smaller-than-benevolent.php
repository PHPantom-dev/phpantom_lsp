<?php

namespace SmallerThanBenevolent;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param int[] $a
	 * @return void
	 */
	public function doFoo(array $a)
	{
		uksort($a, function ($x, $y): int {
			assertType('(int|string)', $x);
			assertType('(int|string)', $y);
			if ($x === 31 || $y === 31) {
				return 1;
			}


			assertType('bool', $x < $y);
			assertType('bool', $x <= $y);
			assertType('bool', $x > $y);
			assertType('bool', $x >= $y);

			return $x < $y ? 1 : -1;
		});
	}

}
