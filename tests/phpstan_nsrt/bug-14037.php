<?php declare(strict_types=1);

namespace Bug14037;

use function PHPStan\Testing\assertType;

/**
 * @param array<10|20|30|'a', mixed> $a
 */
function splice(array $a): void {
	array_splice($a, 0, 0);
	assertType("array<'a'|int<0, max>, mixed>", $a);
}

/**
 * @param array<int<10, 30>|'a', mixed> $a
 */
function splice2(array $a): void {
	array_splice($a, 0, 0);
	assertType("array<'a'|int<0, max>, mixed>", $a);
}
