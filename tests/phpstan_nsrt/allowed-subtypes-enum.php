<?php // lint >= 8.1

namespace AllowedSubtypesEnum;

use function PHPStan\Testing\assertType;

enum Foo {
	case A;
	case B;
	case C;
}

function foo(Foo $foo): void {
	assertType('AllowedSubtypesEnum\\Foo', $foo);

	if ($foo === Foo::B) {
		return;
	}


	if ($foo === Foo::C) {
		return;
	}

	assertType('AllowedSubtypesEnum\\Foo::A', $foo);
}
