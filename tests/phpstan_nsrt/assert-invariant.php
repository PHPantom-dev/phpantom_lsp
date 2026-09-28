<?php // lint >= 8.0

declare(strict_types = 1);

namespace AssertInvariant;

/**
 * @phpstan-assert true $fact
 */
function invariant(bool $fact): void
{
}

function (mixed $m): void {
	invariant(is_bool($m));
	\PHPStan\Testing\assertType('bool', $m);
};

/**
 * @phpstan-assert !false $condition
 */
function assertNotFalse(mixed $condition): void
{
}

function (mixed $m): void {
	assertNotFalse(is_bool($m));
	\PHPStan\Testing\assertType('bool', $m);
};
