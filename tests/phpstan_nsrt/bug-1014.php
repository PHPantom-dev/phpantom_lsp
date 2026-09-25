<?php declare(strict_types=1);

namespace Bug1014;

use function PHPStan\Testing\assertType;

function bug1014(): void {
	$s = rand(0, 1) ? 0 : 1;
	assertType('0|1', $s);
	if ($s) {
		assertType('1', $s);
		$s = 3;
		assertType('3', $s);
	} else {
		assertType('0', $s);
	}
	assertType('0|3', $s);
	if ($s === 1) {
	}
}
