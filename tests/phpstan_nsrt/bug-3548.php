<?php

namespace Bug3548;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	/**
	 * @param int[] $arr
	 */
	public function shift(array $arr): int {
		if (count($arr) === 0) {
			throw new \Exception("oops");
		}
		$name = array_shift($arr);
		assertType('int', $name);
		return $name;
	}
}
