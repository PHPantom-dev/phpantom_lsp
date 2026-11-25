<?php

namespace Bug8084b;

use Exception;
use function array_shift;
use function PHPStan\Testing\assertType;

class Bug8084
{
	/**
	 * @param array{a: 0, b?: 1} $arr
	 * @throws Exception
	 */
	public function run(array $arr): void
	{
		assertType('0', array_shift($arr) ?? throw new Exception()); // PHPStan says `0|1`; PHPantom is more precise here
		assertType('1|null', array_shift($arr));
	}
}
