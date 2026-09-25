<?php

namespace ReusingSpecifiedVariableInForeach;

use function PHPStan\Testing\assertType;

/** @var string|null $business */
$business = doFoo();
if ($business !== null) {
	return;
}

foreach ([1, 2, 3] as $business) {
	assertType('1|2|3', $business);
}
