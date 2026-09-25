<?php // lint >= 8.1

declare(strict_types = 1);

namespace Bug7167;

use function PHPStan\Testing\assertType;

enum Foo {
	case Value;
}

assertType('class-string<Bug7167\Foo>', get_class(Foo::Value));

