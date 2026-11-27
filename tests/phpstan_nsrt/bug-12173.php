<?php // lint >= 7.4

namespace Bug12173;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	public function parse(string $string): void
	{
		$regex = '#.*(?<fruit>(apple|orange)).*#';

		if (preg_match($regex, $string, $matches) !== 1) {
			throw new \Exception('Invalid input');
		}

		assertType("'apple'|'orange'", $matches['fruit']);;
	}
}
