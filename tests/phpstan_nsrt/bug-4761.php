<?php

namespace Bug4761;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	public function sayHello(string $url): void
	{
		if (!\is_string($path = parse_url($url, \PHP_URL_PATH))) {
			return;
		}

		assertType('string', $path);
	}
}
