<?php // lint >= 8.2

namespace Bug8957;

use function PHPStan\Testing\assertType;

enum A: string
{
	case X = 'x';
	case Y = 'y';
}

class B {
	public const A = [
		A::X->value,
		A::Y->value,
	];

	public function doFoo(): void
	{
		assertType('array{\'x\', \'y\'}', self::A); // SKIP: a class constant whose initializer names an enum case is not resolved
	}
}
