<?php declare(strict_types = 1);

namespace ArrayChunk;

use function PHPStan\Testing\assertType;

class Foo
{

	public function generalArrays(array $arr): void
	{
		/** @var mixed[] $arr */
		assertType('list<non-empty-list<mixed>>', array_chunk($arr, 2));
		assertType('list<non-empty-array<mixed>>', array_chunk($arr, 2, true));

		/** @var array<string, int> $arr */
		assertType('list<non-empty-list<int>>', array_chunk($arr, 2));
		assertType('list<non-empty-array<string, int>>', array_chunk($arr, 2, true));

		/** @var non-empty-array<int|string, bool> $arr */
		assertType('non-empty-list<non-empty-list<bool>>', array_chunk($arr, 1));
		assertType('non-empty-list<non-empty-array<int|string, bool>>', array_chunk($arr, 1, true));
	}


	public function constantArrays(array $arr): void
	{
		/** @var array{a: 0, 17: 1, b: 2} $arr */
		assertType('array{array{0, 1}, array{2}}', array_chunk($arr, 2));
		assertType('array{array{a: 0, 17: 1}, array{b: 2}}', array_chunk($arr, 2, true));
		assertType('array{array{0}, array{1}, array{2}}', array_chunk($arr, 1));
		assertType('array{array{a: 0}, array{17: 1}, array{b: 2}}', array_chunk($arr, 1, true));
	}

	public function constantArraysWithOptionalKeys(array $arr): void
	{
		/** @var array{a: 0, b?: 1, c: 2} $arr */
		assertType('array{0: array{a: 0, b?: 1, c?: 2}, 1?: array{c?: 2}}', array_chunk($arr, 2, true)); // PHPStan marks every piece required, but a piece whose entries are all optional may not be there
		assertType('array{array{a: 0, b?: 1, c: 2}}', array_chunk($arr, 3, true));
		assertType('array{0: array{a: 0}, 1?: array{b?: 1, c?: 2}, 2?: array{c?: 2}}', array_chunk($arr, 1, true)); // PHPStan marks every piece required, but a piece whose entries are all optional may not be there

		/** @var array{a?: 0, b?: 1, c?: 2} $arr */
		assertType('array{0?: array{a?: 0, b?: 1, c?: 2}, 1?: array{c?: 2}}', array_chunk($arr, 2, true)); // PHPStan marks every piece required, but a piece whose entries are all optional may not be there
	}

	/**
	 * @param int<2, 3> $positiveRange
	 * @param 2|3 $positiveUnion
	 */
	public function chunkUnionTypeLength(array $arr, $positiveRange, $positiveUnion) {
		/** @var array{a: 0, b?: 1, c: 2} $arr */
		assertType('array{0: list{0: 0, 1?: 1|2, 2?: 2}, 1?: array{0?: 2}}', array_chunk($arr, $positiveRange)); // SKIP: a length range does not cut a constant array into pieces
		assertType('array{0: array{a: 0, b?: 1, c?: 2}, 1?: array{c?: 2}}', array_chunk($arr, $positiveRange, true)); // SKIP: a length range does not cut a constant array into pieces
		assertType('array{0: list{0: 0, 1?: 1|2, 2?: 2}, 1?: array{0?: 2}}', array_chunk($arr, $positiveUnion)); // SKIP: a length that may be several values gives one shape per length, not one joined shape
		assertType('array{0: array{a: 0, b?: 1, c?: 2}, 1?: array{c?: 2}}', array_chunk($arr, $positiveUnion, true)); // SKIP: a length that may be several values gives one shape per length, not one joined shape
	}

	/**
	 * @param positive-int $positiveInt
	 * @param int<50, max> $bigger50
	 */
	public function lengthIntRanges(array $arr, int $positiveInt, int $bigger50) {
		assertType('list<non-empty-list>', array_chunk($arr, $positiveInt));
		assertType('list<non-empty-list>', array_chunk($arr, $bigger50));
	}

	/**
	 * @param int<1, 4> $oneToFour
	 * @param int<1, 5> $tooBig
	 */
	function testLimits(array $arr, int $oneToFour, int $tooBig) {
		/** @var array{a: 0, b?: 1, c: 2, d: 3} $arr */
		assertType('array{0: list{0: 0, 1?: 1|2, 2?: 2|3, 3?: 3}, 1?: list{0?: 2|3, 1?: 3}}|array{array{0}, list{0?: 1|2, 1?: 2}, list{0?: 2|3, 1?: 3}, array{0?: 3}}', array_chunk($arr, $oneToFour)); // SKIP: a length range does not cut a constant array into pieces
		assertType('non-empty-list<non-empty-list<0|1|2|3>>', array_chunk($arr, $tooBig));
	}

	/** @param array<string, string> $map */
	public function offsets(array $arr, array $map): void
	{
		if (array_key_exists('foo', $arr)) {
			assertType('non-empty-list<non-empty-list>', array_chunk($arr, 2));
			assertType('non-empty-list<non-empty-array>', array_chunk($arr, 2, true));
		}
		if (array_key_exists('foo', $arr) && $arr['foo'] === 'bar') {
			assertType('non-empty-list<non-empty-list>', array_chunk($arr, 2));
			assertType('non-empty-list<non-empty-array>', array_chunk($arr, 2, true));
		}

		if (array_key_exists('foo', $map)) {
			assertType('non-empty-list<non-empty-list<string>>', array_chunk($map, 2));
			assertType('non-empty-list<non-empty-array<string, string>>', array_chunk($map, 2, true));
		}
		if (array_key_exists('foo', $map) && $map['foo'] === 'bar') {
			assertType('non-empty-list<non-empty-list<string>>', array_chunk($map, 2));
			assertType('non-empty-list<non-empty-array<string, string>>', array_chunk($map, 2, true));
		}
	}

}
