<?php declare(strict_types = 1);

namespace Bug11854;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	public function sayHello(): void
	{
		$arr = [];
		$arr[] = rand(0,1) ? 'A' : 'B';
		$arr[] = rand(0,1) ? 'C' : '';

		assertType("array{'A'|'B', ''|'C'}", $arr);
		assertType("'A '|'A C'|'B '|'B C'", implode(' ', $arr));
	}
}
