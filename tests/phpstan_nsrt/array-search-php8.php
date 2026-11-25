<?php // lint >= 8.0

declare(strict_types = 1);

namespace ArraySearchPhp8;

use function PHPStan\Testing\assertType;

class Foo
{

	public function mixedAndSubtractedArray($mixed, string $string): void
	{
		if (is_array($mixed)) {
			assertType('int|string|false', array_search('foo', $mixed, true));
			assertType('int|string|false', array_search('foo', $mixed));
			assertType('int|string|false', array_search($string, $mixed, true));
		} else {
		}
	}

}
