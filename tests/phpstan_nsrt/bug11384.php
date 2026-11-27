<?php declare(strict_types=1);

namespace Bug11384;

use function PHPStan\Testing\assertType;

class Bar
{
	public const VAL = 3;
}

class HelloWorld
{
	public function sayHello(string $s): void
	{
		if (preg_match('{(' . Bar::VAL . ')}', $s, $m)) {
			assertType("array{string, '3'}", $m);
		}
	}
}
