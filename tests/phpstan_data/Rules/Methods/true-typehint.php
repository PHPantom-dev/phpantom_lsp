<?php // lint >= 8.2

namespace NativeTrueType;

use function PHPStan\Testing\assertType;

class Truthy {
	public true $truthy = true;

	public function foo(true $v): true {
		assertType('true', $v);
	}

	function trueUnion(true|null $trueUnion): void
	{
		assertType('true|null', $trueUnion);

		if (is_null($trueUnion)) {
			assertType('null', $trueUnion);
			return;
		}

		if (is_bool($trueUnion)) {
			assertType('true', $trueUnion);
			return;
		}

		assertType('*NEVER*', $trueUnion);
	}

	function trueUnionReturn(): true|null
	{
		if (rand(1, 0)) {
			return true;
		}
		return null;
	}
}

function foo(Truthy $truthy) {
	assertType('true', $truthy->truthy);
	assertType('true', $truthy->foo(true));
	assertType('true|null', $truthy->trueUnionReturn());
}
