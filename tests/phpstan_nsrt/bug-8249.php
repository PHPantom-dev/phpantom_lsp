<?php // lint >= 8.0

namespace Bug8249;

use function PHPStan\Testing\assertType;

function foo(): mixed
{
	return null;
}

function () {
	$x = foo();

	if (is_int($x)) {
		assertType('int', $x);
		assertType('true', is_int($x));
	} else {
		assertType('false', is_int($x));
	}
};

function () {
	$x = ['x' => foo()];

	if (is_int($x['x'])) {
		assertType('array{x: int}', $x);
		assertType('int', $x['x']);
		assertType('true', is_int($x['x']));
	} else {
		assertType('false', is_int($x['x']));
	}
};
