<?php

namespace Bug9403;

use function PHPStan\Testing\assertNativeType;
use function PHPStan\Testing\assertType;

class HelloWorld
{

	/**
	 * @param int $max
	 * @return int[]
	 */
	public function testMe(int $max): array
	{
		$result = [];
		for ($i = 0; $i < $max; $i++) {
			array_push($result, $i);
		}

		assertType('list<int>', $result); // SKIP: array_push() does not change the array's type
		assertNativeType('list<int<0, max>>', $result);

		if (!empty($result)) {
			rsort($result);
		}
		return $result;
	}

}
