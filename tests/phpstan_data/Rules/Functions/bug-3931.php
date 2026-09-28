<?php

namespace Bug3931;

use function PHPStan\Testing\assertType;

/**
 * @template T of array
 * @param T $arr
 * @return T & array{mykey: int}
 */
function addSomeKey(array $arr, int $value): array {
	$arr['mykey'] = $value;
	return $arr;
}

/**
 * @param array<string> $arr
 * @return void
 */
function test(array $arr): void
{
	$r = addSomeKey($arr, 1);
	// PHPantom is more precise than PHPStan here: it keeps the `T` half of `T & array{mykey: int}`, which the upstream comment says PHPStan loses.
	assertType('array<string>&array{mykey: int}', $r); // could be better, the T part currently disappears
}
