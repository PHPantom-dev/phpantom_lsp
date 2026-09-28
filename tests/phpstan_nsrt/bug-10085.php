<?php

declare(strict_types = 1);

namespace Bug10085;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	/**
	 * @param array<int, string> $foo
	 * @param list<string> $bar
	 */
	public function sayHello(array $foo, array $bar): void
	{
		$a = $foo;
		if ($a === []) {
			$a = $bar;
		}

		if ($a === []) {
			return;
		}

		assertType('array<int, string>', $foo);
	}
}
