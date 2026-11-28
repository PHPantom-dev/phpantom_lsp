<?php // lint >= 8.2

namespace EnumReflection82;

use ReflectionEnum;
use ReflectionEnumBackedCase;
use ReflectionEnumUnitCase;
use function PHPStan\Testing\assertType;

enum Foo: int
{

	case FOO = 1;
	case BAR = 2;
}

function testNarrowGetBackingTypeAfterIsBacked() {
	$r = new ReflectionEnum(Foo::class);
	assertType('ReflectionNamedType|null', $r->getBackingType());
	if ($r->isBacked()) {
		assertType('ReflectionNamedType', $r->getBackingType());
	}
}
