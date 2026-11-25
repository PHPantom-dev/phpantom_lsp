<?php declare(strict_types = 1);

namespace Bug8373;

use function PHPStan\Testing\assertType;

function (string $one, string $two): string {
	$args = [$one, $two];

	assertType('array{string, string}', $args);
	assertType('array{0?: non-falsy-string, 1?: non-falsy-string}', array_filter($args));

	\assert(array_filter($args) === $args);

	assertType('array{non-falsy-string, non-falsy-string}', array_filter($args)); // SKIP: comparing an array with `array_filter()` of itself narrows nothing

	return $args[1];
};
