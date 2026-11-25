<?php declare(strict_types = 1);

namespace Bug7501;

use function PHPStan\Testing\assertType;

/**
 * @param array{key?: mixed} $a
 */
function f(array $a): void
{
	if (isset($a['key']) && is_int($a['key'])) {
		assertType('array{key: int}', $a);
	}

	if (is_int($a['key'] ?? null)) {
		assertType('array{key: int}', $a);
	}
}
