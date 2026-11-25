<?php // lint >= 8.0

namespace SubtractMixed;

use function PHPStan\Testing\assertType;

/**
 * @param int|0.0|''|'0'|array{}|false|null $moreThenFalsy
 */
function subtract(mixed $m, $moreThenFalsy) {
	if ($m !== true) {
		assertType('bool', (bool) $m); // mixed could still contain something truthy
	}
	if ($m !== false) {
		assertType('bool', (bool) $m); // mixed could still contain something falsy
	}
	if (!is_bool($m)) {
		assertType('bool', (bool) $m);
	}
	if (!is_array($m)) {
		assertType('bool', (bool) $m);
	}

	if ($m) {
	}
	if (!$m) {
		assertType("0|0.0|''|'0'|array{}|false|null", $m);
	}
	if (!$m) {
		if (!is_int($m)) {
			assertType("0.0|''|'0'|array{}|false|null", $m);
		}
		if (!is_bool($m)) {
			assertType("0|0.0|''|'0'|array{}|null", $m);
		}
	}

	if (!$m || is_int($m)) {
		assertType('bool', (bool) $m);
	}

	if ($m !== $moreThenFalsy) {
		assertType('mixed', $m);
		assertType('bool', (bool) $m); // could be true
	}

	if ($m != 0 && !is_array($m) && $m != null && !is_object($m)) { // subtract more types then falsy
	}
}
