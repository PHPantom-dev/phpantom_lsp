<?php // lint >= 8.1

namespace PHPStan\Generics\GenericEnumClassStringType;

use function PHPStan\Testing\assertType;

function testEnumExists(string $str)
{
	assertType('string', $str);
	if (enum_exists($str)) {
		assertType('class-string<UnitEnum>', $str);
	}
}

