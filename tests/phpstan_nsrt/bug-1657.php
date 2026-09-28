<?php

namespace Bug1657;

use function PHPStan\Testing\assertType;

/**
 * @param string|int $value
 */
function foo($value)
{
	assertType('int|string', $value);
	try {
		assert(is_string($value));
		assertType('string', $value);

	} catch (\Throwable $e) {
		$value = 'A';
		assertType('\'A\'', $value);
	}

	assertType('string', $value);
};
