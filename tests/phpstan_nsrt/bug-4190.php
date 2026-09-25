<?php

namespace Bug4190;

use function PHPStan\Testing\assertType;

function (): string
{
	if (random_int(0, 10) <= 5) {
		return 'first try';
	}

	assertType('bool', random_int(0, 10) <= 5);

	return 'foo';
};
