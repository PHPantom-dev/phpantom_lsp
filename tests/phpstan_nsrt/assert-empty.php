<?php // lint >= 8.0

namespace AssertEmpty;

use function PHPStan\Testing\assertType;

/**
 * @phpstan-assert empty $var
 */
function assertEmpty(mixed $var): void
{
}

/**
 * @phpstan-assert !empty $var
 */
function assertNotEmpty(mixed $var): void
{
}

function ($var) {
	assertEmpty($var);
	assertType("0|0.0|''|'0'|array{}|false|null", $var);
};

function ($var) {
	assertNotEmpty($var);
	assertType("mixed~(0|0.0|''|'0'|array{}|false|null)", $var);
};
