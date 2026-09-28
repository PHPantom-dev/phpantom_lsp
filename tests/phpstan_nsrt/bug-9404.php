<?php

namespace Bug9404;

use function PHPStan\Testing\assertType;

function foo(int|string $x): void
{
	if (gettype($x) === 'integer') {
		assertType("'integer'", gettype($x));
	}
}
