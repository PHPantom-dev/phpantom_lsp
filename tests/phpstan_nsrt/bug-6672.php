<?php declare(strict_types = 1);

namespace Bug6672;

use function PHPStan\Testing\assertType;

function foo(int $a, ?int $b, int $c, ?int $d, ?int $e, ?int $f): void
{
	if ($a > 17) {
		assertType('int<18, max>', $a);
	} else {
		assertType('int<min, 17>', $a);
	}

	if ($b > 17 || $b === null) {
		assertType('int<18, max>|null', $b);
	} else {
		assertType('int<min, 17>', $b);
	}

	if ($c < 17) {
		assertType('int<min, 16>', $c);
	} else {
		assertType('int<17, max>', $c);
	}

	if ($d < 17 || $d === null) {
	} else {
		assertType('int<17, max>', $d);
	}

	if ($e >= 17 && $e <= 19 || $e === null) {
		assertType('int<17, 19>|null', $e);
	} else {
		assertType('int<min, 16>|int<20, max>', $e);
	}

	if ($f < 17 || $f > 19 || $f === null) {
	} else {
		assertType('int<17, 19>', $f);
	}
}
