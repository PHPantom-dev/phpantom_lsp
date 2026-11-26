<?php

namespace ArraySlice;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param non-empty-array $a
	 * @param non-empty-list $b
	 * @param non-empty-array<int>|non-empty-list<string> $c
	 */
	public function nonEmpty(array $a, array $b, array $c): void
	{
		assertType('array', array_slice($a, 1));
		assertType('list', array_slice($b, 1));
		assertType('array<int>|list<string>', array_slice($c, 1)); // PHPStan joins the two into `array<int|string>`
	}

	/**
	 * @param mixed $arr
	 */
	public function fromMixed($arr): void
	{
		assertType('array<mixed, mixed>', array_slice($arr, 1, 2));
	}

	public function normalArrays(array $arr): void
	{
		/** @var array<int, bool> $arr */
		assertType('list<bool>', array_slice($arr, 1, 2));
		assertType('array<int, bool>', array_slice($arr, 1, 2, true));

		/** @var array<string, int> $arr */
		assertType('array<string, int>', array_slice($arr, 1, 2));
		assertType('array<string, int>', array_slice($arr, 1, 2, true));

		/** @var non-empty-array<string> $arr */
		assertType('array{}', array_slice($arr, 0, 0));
		assertType('array{}', array_slice($arr, 0, 0, true));

		/** @var non-empty-array<string> $arr */
		assertType('non-empty-array<string>', array_slice($arr, 0, 1)); // PHPStan says `array<string>`, but one entry from the start of a non-empty array is there
		assertType('non-empty-array<string>', array_slice($arr, 0, 1, true)); // PHPStan says `array<string>`, but one entry from the start of a non-empty array is there

		/** @var list<string> $arr */
		assertType('list<string>', array_slice($arr, 0, 1));
		assertType('list<string>', array_slice($arr, 0, 1, true));

		/** @var non-empty-list<string> $arr */
		assertType('non-empty-list<string>', array_slice($arr, 0, 1));
		assertType('non-empty-list<string>', array_slice($arr, 0, 1, true));
	}

	public function constantArrays(array $arr): void
	{
		/** @var array{17: 'foo', b: 'bar', 19: 'baz'} $arr */
		assertType('array{b: \'bar\', 0: \'baz\'}', array_slice($arr, 1, 2));
		assertType('array{b: \'bar\', 19: \'baz\'}', array_slice($arr, 1, 2, true));
		assertType('array<int|\'b\', \'bar\'|\'baz\'|\'foo\'>', array_slice($arr, rand(0, 1) ? 0 : 1, rand(0, 1) ? 0 : 1)); // PHPStan says `array<17|19|'b', …>`, but the int keys are renumbered

		/** @var array{17: 'foo', 19: 'bar', 21: 'baz'}|array{foo: 17, bar: 19, baz: 21} $arr */
		assertType('array{\'bar\', \'baz\'}|array{bar: 19, baz: 21}', array_slice($arr, 1, 2));
		assertType('array{19: \'bar\', 21: \'baz\'}|array{bar: 19, baz: 21}', array_slice($arr, 1, 2, true));

		/** @var array{17: 'foo', b: 'bar', 19: 'baz'} $arr */
		assertType('array{}', array_slice($arr, -1, -1));
		assertType('array{}', array_slice($arr, -1, -1, true));

		/** @var array{17: 'foo', b: 'bar', 19: 'baz'} $arr */
		assertType('array{}', array_slice($arr, -1, -2));
		assertType('array{}', array_slice($arr, -1, -2, true));
	}

	public function constantArraysWithOptionalKeys(array $arr): void
	{
		/** @var array{a?: 0, b: 1, c: 2} $arr */
		assertType('array{a?: 0, b?: 1}', array_slice($arr, 0, 1));
		assertType('array{a?: 0, b: 1, c: 2}', array_slice($arr, 0));
		assertType('array{a?: 0, b: 1, c: 2}', array_slice($arr, -99));
		assertType('array{a?: 0, b: 1, c: 2}', array_slice($arr, 0, 99));
		assertType('array{a?: 0}', array_slice($arr, 0, -2));
		assertType('array{}', array_slice($arr, 0, -3));
		assertType('array{}', array_slice($arr, 0, -99));
		assertType('array{}', array_slice($arr, -99, -99));
		assertType('array{}', array_slice($arr, 99));

		/** @var array{a?: 0, b?: 1, c: 2, d: 3, e: 4} $arr */
		assertType('array{c?: 2, d?: 3, e?: 4}', array_slice($arr, 2, 1));
		assertType('array{b?: 1, c?: 2, d: 3, e?: 4}', array_slice($arr, 1, 3));
		assertType('array{e: 4}', array_slice($arr, -1));
		assertType('array{d: 3}', array_slice($arr, -2, 1));

		/** @var array{a: 0, b: 1, c: 2, d?: 3, e?: 4} $arr */
		assertType('array{c: 2}', array_slice($arr, 2, 1));
		assertType('array{c?: 2, d?: 3, e?: 4}', array_slice($arr, -1));
		assertType('array{b?: 1, c?: 2, d?: 3}', array_slice($arr, -2, 1));
		assertType('array{a: 0, b: 1, c?: 2, d?: 3}', array_slice($arr, 0, -1));
		assertType('array{a: 0, b?: 1, c?: 2}', array_slice($arr, 0, -2));

		/** @var array{a: 0, b?: 1, c: 2, d?: 3, e: 4} $arr */
		assertType('array{b?: 1, c: 2, d?: 3, e?: 4}', array_slice($arr, 1, 2));
		assertType('array{a: 0, b?: 1, c?: 2}', array_slice($arr, 0, 2));
		assertType('array{a: 0}', array_slice($arr, 0, 1));
		assertType('array{b?: 1, c?: 2}', array_slice($arr, 1, 1));
		assertType('array{c?: 2, d?: 3, e?: 4}', array_slice($arr, 2, 1));
		assertType('array{a: 0, b?: 1, c: 2, d?: 3}', array_slice($arr, 0, -1));
		assertType('array{c?: 2, d?: 3, e: 4}', array_slice($arr, -2));

		/** @var array{a: 0, b?: 1, c: 2} $arr */
		assertType('array{a: 0, b?: 1}', array_slice($arr, 0, -1));
		assertType('array{a: 0}', array_slice($arr, -3, 1));
	}


	public function offsets(array $arr): void
	{
		if (array_key_exists(1, $arr)) {
			assertType('array', array_slice($arr, 1, null, false)); // PHPStan says non-empty, but `[1 => 'x']` has key 1 and slices to `[]`
			assertType('array', array_slice($arr, 1, null, true)); // PHPStan says non-empty, but `[1 => 'x']` has key 1 and slices to `[]`
		}
		if (array_key_exists(1, $arr) && $arr[1] === 'foo') {
			assertType('array', array_slice($arr, 1, null, false)); // PHPStan says non-empty, but `[1 => 'x']` has key 1 and slices to `[]`
			assertType('array', array_slice($arr, 1, null, true)); // PHPStan says non-empty, but `[1 => 'x']` has key 1 and slices to `[]`
		}
	}

}
