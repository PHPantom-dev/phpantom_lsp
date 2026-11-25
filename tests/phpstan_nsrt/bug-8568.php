<?php declare(strict_types = 1);

namespace Bug8568;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	public function sayHello(): void
	{
	}

	public function get(): ?int
	{
		return rand() ? 5 : null;
	}

	/**
	 * @param numeric-string $numericS
	 */
	public function intersections($numericS): void {
		assertType('non-falsy-string', 'a'. $numericS);
		assertType('numeric-string', (string) $numericS);
	}
}
