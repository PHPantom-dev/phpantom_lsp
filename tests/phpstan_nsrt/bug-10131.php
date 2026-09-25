<?php // lint >= 8.0

namespace Bug10131;

use function PHPStan\Testing\assertType;

class A {
}

class B {
	public A|null $a = null;
}

/**
 * @phpstan-return array{0:A|null, 1:B|null}
 */
function foo(A|null $a, B|null $b): array
{
	$a ??= $b?->a ?? throw new \Exception();

	assertType(A::class, $a);
	assertType(B::class . '|null', $b);

	return [$a, $b];
}
