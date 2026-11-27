<?php

namespace Bug11692;

use function PHPStan\Testing\assertType;

/** @param int|float $floatOrInt */
function doFoo(int $i, float $f, $floatOrInt): void {
	assertType('non-empty-list<float>', range(1, 9, .01));
	assertType('array{1, 4, 7}', range(1, 9, 3));

	assertType('non-empty-list<float>', range(1, 9999, .01));
	assertType('non-empty-list<int<1, 9999>>', range(1, 9999, 3));

	assertType('non-empty-list<float|int>', range(1, 9999, $floatOrInt));

	assertType('non-empty-list<int<1, 3>>', range(1, 3, $i)); // PHPantom is more precise: every value lies between the bounds
	assertType('non-empty-list<float|int>', range(1, 3, $f));

	assertType('non-empty-list<int<1, 9999>>', range(1, 9999, $i)); // PHPantom is more precise: every value lies between the bounds
	assertType('non-empty-list<float|int>', range(1, 9999, $f));
}

