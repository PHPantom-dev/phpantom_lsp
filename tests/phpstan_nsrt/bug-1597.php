<?php

namespace Bug1597;

use function PHPStan\Testing\assertType;

$date = '';

try {
	if (rand(0,1) === 0) {
		throw new \Exception();
	}
	$date = new \DateTime($date);
} catch (\Exception $e) {
	assertType('\'\'', $date);
}
