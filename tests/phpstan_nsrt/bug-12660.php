<?php declare(strict_types = 1);

namespace Bug12660;

use function PHPStan\Testing\assertType;

class Integer
{
	
}

class HelloWorld
{
	/**
	 * @param Integer $integer
	 */
	public function sayHello($integer): void
	{
		assertType(Integer::class, $integer);
	}
}
