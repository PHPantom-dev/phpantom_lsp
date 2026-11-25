<?php

namespace Bug4206;

use function PHPStan\Testing\assertType;

class Foo
{

	public const ONE = 1;
	public const TWO = 2;

}

function (int $i): void {
	if ($i === Foo::ONE) {
		assertType('1', $i);
	}

	if ($i === Foo::TWO) {
		assertType('2', $i);
	}
};
