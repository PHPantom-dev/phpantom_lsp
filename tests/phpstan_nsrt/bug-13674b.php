<?php

namespace Bug13674b;

use function assert;
use function PHPStan\Testing\assertType;

class HelloWorld
{
	/**
	 * @param array<int> $arrayA
	 * @param list<int> $listA
	 */
	public function sayHello($arrayA, $listA, int $i): void
	{
		if (isset($arrayA[$i])) {
		} else {
			assertType('array<int>', $arrayA);
		}
		assertType('array<int>', $arrayA);

		if (isset($listA[$i])) {
		} else {
			assertType('list<int>', $listA);
		}
		assertType('list<int>', $listA);

		if (!isset($listA[$i])) {
			assertType('list<int>', $listA);
			return;
		}

		$emptyArray = [];
	}
}
