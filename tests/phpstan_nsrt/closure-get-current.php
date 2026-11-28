<?php // lint >= 8.5

namespace ClosureGetCurrent;

use Closure;
use function PHPStan\Testing\assertType;

function doFoo(): void {
	assertType('*NEVER*', Closure::getCurrent());
}

function (int $i): string {
	assertType("Closure(int): 'foo'", Closure::getCurrent());

	return 'foo';
};
