<?php

namespace Bug6696;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	public function sayHello(?string $s): void
	{
		assert($s !== '');
		assertType('non-empty-string|null', $s);
	}

	public function sayHello2(?string $s): void
	{
		assert($s === null || $s !== '');
		assertType('non-empty-string|null', $s);
	}
}
