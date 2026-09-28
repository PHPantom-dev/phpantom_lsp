<?php

namespace Bug4326;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	public function sayHello(?string $a, ?string $b): string
	{
		if (null === $a && null === $b) {
			return 'no $a, no $b';
		}
		assertType('string|null', $a);
		assertType('string|null', $b);
		if (null === $a) {
			assertType('null', $a);
			assertType('string', $b);
			return $b;
		}

		assertType('string', $a);
		assertType('string|null', $b);

		if (null === $b) {
			return $a;
		}

		assertType('string', $a);
		assertType('string', $b);

		return $a . ' - ' . $b;
	}
}
