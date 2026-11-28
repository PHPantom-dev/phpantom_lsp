<?php // lint >= 8.1

namespace EnumReflection;

use ReflectionEnum;
use ReflectionEnumBackedCase;
use ReflectionEnumUnitCase;
use function PHPStan\Testing\assertType;

enum Foo: int
{

	case FOO = 1;
	case BAR = 2;

	public function doFoo(): void
	{
		$r = new ReflectionEnum(self::class);
		foreach ($r->getCases() as $case) {
			assertType(ReflectionEnumBackedCase::class, $case);
		}

		assertType(ReflectionEnumBackedCase::class, $r->getCase('FOO'));
	}

}

enum Bar
{

	case FOO;
	case BAR;

	public function doFoo(): void
	{
		$r = new ReflectionEnum(self::class);
		foreach ($r->getCases() as $case) {
			assertType(ReflectionEnumUnitCase::class, $case);
		}
		assertType(ReflectionEnumUnitCase::class, $r->getCase('FOO'));
	}

}
