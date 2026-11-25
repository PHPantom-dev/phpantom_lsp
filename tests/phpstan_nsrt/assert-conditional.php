<?php // lint >= 8.0

namespace AssertConditional;

use function PHPStan\Testing\assertType;

/**
 * @phpstan-assert ($if is true ? true : false) $condition
 */
function assertIf(mixed $condition, bool $if)
{
}

function (mixed $value1, mixed $value2) {
	assertIf($value1, true);
	assertType('true', $value1);

	assertIf($value2, false);
	assertType('false', $value2);
};

/**
 * @template T of bool
 * @param T $if
 * @phpstan-assert (T is true ? true : false) $condition
 */
function assertIfTemplated(mixed $condition, bool $if)
{
}

function (mixed $value1, mixed $value2) {
	assertIfTemplated($value1, true);
	assertType('true', $value1);

	assertIfTemplated($value2, false);
	assertType('false', $value2);
};
