<?php

namespace ArrayIsListUnset;

use function PHPStan\Testing\assertType;

function () {
	$a = [];
	assertType('array{}', $a);
	$a[] = 1;
	assertType('array{1}', $a);
	$a[] = 2;
	assertType('array{1, 2}', $a);
	$a[] = 3;
	assertType('array{1, 2, 3}', $a);
	unset($a[2]);
	assertType('array{1, 2}', $a);
	$a[] = 4;
	assertType('array{0: 1, 1: 2, 3: 4}', $a); // SKIP: an append after `unset()` of the last entry reuses its index
};
