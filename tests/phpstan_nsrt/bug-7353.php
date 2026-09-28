<?php declare(strict_types = 1);

namespace Bug7353;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	/** @param array<literal-string, mixed> $data */
	public function sayHello(array $data): void
	{
		assertType('array<literal-string, mixed>', $data);
	}
}
