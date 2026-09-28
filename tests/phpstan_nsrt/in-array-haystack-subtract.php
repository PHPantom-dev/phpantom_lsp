<?php

namespace InArrayHaystackSubtract;

use function PHPStan\Testing\assertType;

class Foo
{

	/** @param array<int> $haystack */
	public function specifyHaystack(array $haystack): void
	{
		if (! in_array(rand() ? 5 : 6, $haystack, true)) {
			assertType('array<int>', $haystack);
		}
	}

}
