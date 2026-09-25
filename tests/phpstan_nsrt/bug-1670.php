<?php

namespace Bug1670;

use function PHPStan\Testing\assertType;

/** @var \stdClass|null */
$object = null;

try {
	if ($object === null) {
		throw new \InvalidArgumentException();
	}

	throw new \RuntimeException();
} catch (\Throwable $e) {
	assertType('stdClass|null', $object);
}
