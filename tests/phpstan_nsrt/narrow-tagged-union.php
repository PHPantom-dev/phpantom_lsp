<?php declare(strict_types=1);

namespace NarrowTaggedUnion;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	/** @param array{string, '', non-empty-string}|array{string, numeric-string} $arr */
	public function sayHello(array $arr): void
	{
		if (count($arr) === 0) {
			assertType('*NEVER*', $arr);
		} else {
			assertType("array{string, '', non-empty-string}|array{string, numeric-string}", $arr);
		}
		assertType("array{string, '', non-empty-string}|array{string, numeric-string}", $arr);

		if (count($arr) === 1) {
			assertType('*NEVER*', $arr);
		} else {
			assertType("array{string, '', non-empty-string}|array{string, numeric-string}", $arr);
		}
		assertType("array{string, '', non-empty-string}|array{string, numeric-string}", $arr);

		if (count($arr) === 2) {
			assertType('array{string, numeric-string}', $arr);
		} else {
			assertType("array{string, '', non-empty-string}", $arr);
		}
		assertType("array{string, '', non-empty-string}|array{string, numeric-string}", $arr);

		if (count($arr) === 3) {
			assertType("array{string, '', non-empty-string}", $arr);
		} else {
			assertType('array{string, numeric-string}', $arr);
		}
		assertType("array{string, '', non-empty-string}|array{string, numeric-string}", $arr);

		if (count($arr, COUNT_NORMAL) === 3) {
			assertType("array{string, '', non-empty-string}", $arr);
		} else {
			assertType('array{string, numeric-string}', $arr);
		}
		assertType("array{string, '', non-empty-string}|array{string, numeric-string}", $arr);

		if (count($arr, COUNT_RECURSIVE) === 3) {
			assertType("array{string, '', non-empty-string}", $arr);
		} else {
			assertType('array{string, numeric-string}', $arr);
		}
		assertType("array{string, '', non-empty-string}|array{string, numeric-string}", $arr);

		if (count($arr) === 4) {
			assertType('*NEVER*', $arr);
		} else {
			assertType("array{string, '', non-empty-string}|array{string, numeric-string}", $arr);
		}
		assertType("array{string, '', non-empty-string}|array{string, numeric-string}", $arr);

	}

	/** @param array{string, '', non-empty-string}|array{array<int>, numeric-string} $arr */
	public function nestedArrays(array $arr): void
	{
		// don't narrow when $arr contains recursive arrays
		if (count($arr, COUNT_RECURSIVE) === 3) {
			assertType("array{array<int>, numeric-string}|array{string, '', non-empty-string}", $arr);
		} else {
			assertType("array{array<int>, numeric-string}|array{string, '', non-empty-string}", $arr);
		}
		assertType("array{array<int>, numeric-string}|array{string, '', non-empty-string}", $arr);

		if (count($arr, COUNT_NORMAL) === 3) {
			assertType("array{string, '', non-empty-string}", $arr);
		} else {
			assertType("array{array<int>, numeric-string}", $arr);
		}
		assertType("array{array<int>, numeric-string}|array{string, '', non-empty-string}", $arr);
	}

	/** @param array{string, '', non-empty-string}|array<int> $arr */
	public function mixedArrays(array $arr): void
	{
		if (count($arr, COUNT_NORMAL) === 3) {
			assertType("array{string, '', non-empty-string}|array<int>", $arr); // PHPStan says `non-empty-array<int|string>`; PHPantom keeps the members, as its comment suggests
		} else {
			assertType("array<int>", $arr); // PHPStan says `array<int|string>`; the shape always counts 3, so only `array<int>` is left
		}
		assertType("array{string, '', non-empty-string}|array<int>", $arr); // PHPStan says `array<int|string>`; PHPantom keeps the members, as its comment suggests
	}

	public function arrayIntRangeSize(): void
	{
		$x = [];
		if (rand(0,1)) {
			$x[] = 'ab';
		}
		if (rand(0,1)) {
			$x[] = 'xy';
		}

		if (count($x) === 1) {
			assertType("array{'ab'|'xy'}", $x); // PHPStan says `array{'xy'}|array{0: 'ab', 1?: 'xy'}`, the same arrays once the count is 1
		} else {
			assertType("array{}|array{0: 'ab', 1?: 'xy'}", $x); // SKIP: conditional appends forget which value sits at which offset
		}
		assertType("array{}|array{'xy'}|array{0: 'ab', 1?: 'xy'}", $x); // SKIP: conditional appends forget which value sits at which offset
	}
}

