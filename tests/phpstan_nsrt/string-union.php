<?php declare(strict_types = 1);

namespace StringUnion;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	/**
	 * @param literal-string $s1
	 * @param numeric-string $s2
	 */
	public function sayHello(string $s1, string $s2): void
	{
		$a = random_int(0, 1) ? $s1 : $s2;
		assertType('literal-string|numeric-string', $a);
		$b = random_int(0, 1) ? $s2 : $s1;
		assertType('literal-string|numeric-string', $b);
	}
}
