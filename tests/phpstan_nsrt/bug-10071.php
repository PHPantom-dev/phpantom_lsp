<?php // lint >= 8.0

declare(strict_types=1);

namespace Bug10071;

use function PHPStan\Testing\assertType;

class Foo
{
	public ?bool $bar = null;
}


function okIfBar(?Foo $foo = null): void
{
	if ($foo?->bar !== false) {
		assertType(Foo::class . '|null', $foo);
	} else {
		assertType(Foo::class, $foo);
	}

	assertType(Foo::class . '|null', $foo);
}
