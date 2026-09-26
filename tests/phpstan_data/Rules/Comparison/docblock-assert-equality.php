<?php

namespace DocblockAssertEquality;

use function PHPStan\Testing\assertType;

/** @phpstan-assert-if-true =int $x */
function equalsRandomInteger($x)
{
	return is_int($x) && $x === random_int(0, 10);
}

/** @phpstan-assert-if-true int $x */
function isAnInteger($x)
{
	return is_int($x);
}

function foo($x): void
{
	if (equalsRandomInteger($x)) {
		assertType('int', $x); // SKIP: an equality assertion (@phpstan-assert-if-true =int) is not read
	} else {
		assertType('mixed', $x);
	}

	if (isAnInteger($x)) {
		assertType('int', $x); // SKIP: @phpstan-assert-if-true does not narrow an untyped parameter
	} else {
	}
}

function bar(int $x): void
{
	if (equalsRandomInteger($x)) {
		assertType('int', $x);
	} else {
		assertType('int', $x);
	}

	if (isAnInteger($x)) {
		assertType('int', $x);
	} else {
		assertType('*NEVER*', $x); // SKIP: exhausted narrowing leaves the last type standing instead of never
	}
}
