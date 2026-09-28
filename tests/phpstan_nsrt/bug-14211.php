<?php // lint >= 8.0

declare(strict_types = 1);

namespace Bug14211;

use function PHPStan\Testing\assertType;

/** @param array<mixed> $data */
function DoSomithing(array $data): bool {

	if (!isset($data['x']))
		return false;

	$m = isset($data['y']);

	if ($m) {
		assertType('true', $m); // ok: true
	}
	assertType('bool', $m); // ok: bool

	if ($m) {
		assertType('true', $m); // <-- should not be: NEVER
	}

	return true;
}
