<?php

namespace Bug6488;

use function PHPStan\Testing\assertType;

function test() {
	$items = array_fill(0, rand(0, 5), rand(0, 100));


	if (sizeof($items) === 0) {
		return false;
	}

	if (sizeof($items) === 1) {
		return false;
	}

	foreach ($items as $key => $value) {
		if ($value % 2 === 0) {
			unset($items[$key]);
		}
	}

	assertType('bool',sizeof($items) > 0);
}
