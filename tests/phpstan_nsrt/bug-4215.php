<?php

namespace Bug4215;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param int|null $a
	 * @param array<mixed> $b
	 */
	function test(int $a = null, array $b = null): void
	{
		if ($a === null && $b === null) return;

		if ($b === null) {
			assertType('int', $a);
		}
	}

}
