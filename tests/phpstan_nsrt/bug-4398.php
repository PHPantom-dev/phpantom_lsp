<?php

namespace Bug4398;

use function PHPStan\Testing\assertType;

function (array $meters): void {
	assertType('array', $meters);
	if (empty($meters)) {
		throw new \Exception('NO_METERS_FOUND');
	}

	assertType('non-empty-array', $meters);
	assertType('array', array_reverse());
};
