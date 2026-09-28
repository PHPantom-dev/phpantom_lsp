<?php

namespace Bug5309;

use function PHPStan\Testing\assertType;

function greater(float $y): float {
	$x = 0.0;
	if($y > 0) {
		$x += 1;
	}
	assertType('0.0|1.0', $x);
	if($x > 0) {
		assertType('1.0', $x);
		return 5 / $x;
	}
	assertType('0.0', $x); // PHPStan keeps `0.0|1.0`, but `1.0 > 0` returned above

	return 1.0;
}

function greaterEqual(float $y): float {
	$x = 0.0;
	if($y > 0) {
		$x += 1;
	}
	assertType('0.0|1.0', $x);
	if($x >= 0) {
		assertType('0.0|1.0', $x);
		return 5 / $x;
	}
	assertType('*NEVER*', $x); // PHPStan keeps `0.0|1.0`, but both are `>= 0` and returned above

	return 1.0;
}

function smaller(float $y): float {
	$x = 0.0;
	if($y > 0) {
		$x -= 1;
	}
	assertType('-1.0|0.0', $x);
	if($x < 0) {
		assertType('-1.0', $x);
		return 5 / $x;
	}
	assertType('0.0', $x); // PHPStan keeps `-1.0|0.0`, but `-1.0 < 0` returned above

	return 1.0;
}

function smallerEqual(float $y): float {
	$x = 0.0;
	if($y > 0) {
		$x -= 1;
	}
	assertType('-1.0|0.0', $x);
	if($x <= 0) {
		assertType('-1.0|0.0', $x);
		return 5 / $x;
	}
	assertType('*NEVER*', $x);

	return 1.0;
}
