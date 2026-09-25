<?php

namespace OffsetValueAfterAssign;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param array<string> $a
	 */
	public function doFoo(array $a, int $i): void
	{
		assertType('string', $a[$i]);

		$a[$i] = 'foo';
		assertType('\'foo\'', $a[$i]);

		$i = 1;
		assertType('string', $a[$i]);
	}

}
