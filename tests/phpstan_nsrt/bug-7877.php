<?php declare(strict_types=1);

namespace Bug7877;

use function PHPStan\Testing\assertType;

function foo(string $s): void
{
	if ('' !== $s) {
		assertType('non-empty-string', $s);

		if ('0' !== $s) {
		}
	}

	if ('0' !== $s) {
		assertType('string', $s);
	} else {
		assertType("'0'", $s);
	}
	assertType('string', $s);
}

