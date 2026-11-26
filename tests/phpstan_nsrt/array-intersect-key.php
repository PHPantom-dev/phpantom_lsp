<?php

namespace ArrayIntersectKey;

use function array_intersect_key;
use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param non-empty-array<int|string, string> $arr
	 * @param non-empty-array<int, string> $arr2
	 */
	public function nonEmpty(array $arr, array $arr2): void
	{
		assertType('non-empty-array<int|string, string>', array_intersect_key($arr));
		assertType('array<int|string, string>', array_intersect_key($arr, $arr));
		assertType('array<int, string>', array_intersect_key($arr, $arr2));
		assertType('array<int, string>', array_intersect_key($arr2, $arr));
		assertType('array{}', array_intersect_key($arr, []));
		assertType("array<'foo', string>", array_intersect_key($arr, ['foo' => 17]));
	}


	/**
	 * @param array<int|string, string> $arr
	 * @param array<int, string> $arr2
	 */
	public function normalArrays(array $arr, array $arr2, array $otherArrs): void
	{
		assertType('array<int|string, string>', array_intersect_key($arr));
		assertType('array<int|string, string>', array_intersect_key($arr, $arr));
		assertType('array<int, string>', array_intersect_key($arr, $arr2));
		assertType('array<int, string>', array_intersect_key($arr2, $arr));
		assertType('array{}', array_intersect_key($arr, []));
		assertType("array<'foo', string>", array_intersect_key($arr, ['foo' => 17]));

		/** @var array<int, string> $otherArrs */
		assertType('array<int, string>', array_intersect_key($arr, $otherArrs));
		/** @var array<int|string, int|string> $otherArrs */
		assertType('array<int|string, string>', array_intersect_key($arr, $otherArrs));
		/** @var array<string, int> $otherArrs */
		assertType('array<string, string>', array_intersect_key($arr, $otherArrs));
		/** @var array<17, int> $otherArrs */
		assertType('array<17, string>', array_intersect_key($arr, $otherArrs));
		/** @var array<null, int> $otherArrs */
		assertType('array<\'\', string>', array_intersect_key($arr, $otherArrs));

		if (array_key_exists(17, $arr2)) {
			assertType('non-empty-array<17, string>&hasOffset(17)', array_intersect_key($arr2, [17 => 'bar'])); // SKIP: a key `array_key_exists()` proved does not survive `array_intersect_key()`
			/** @var array<int, string> $otherArrs */
			assertType('array<int, string>', array_intersect_key($arr2, $otherArrs));
			/** @var array<string, int> $otherArrs */
			assertType('array{}', array_intersect_key($arr2, $otherArrs));
		}

		if (array_key_exists(17, $arr2) && $arr2[17] === 'foo') {
			assertType("non-empty-array<17, string>&hasOffsetValue(17, 'foo')", array_intersect_key($arr2, [17 => 'bar'])); // SKIP: a key `array_key_exists()` proved does not survive `array_intersect_key()`
			/** @var array<int, string> $otherArrs */
			assertType('array<int, string>', array_intersect_key($arr2, $otherArrs));
			/** @var array<string, int> $otherArrs */
			assertType('array{}', array_intersect_key($arr2, $otherArrs));
		}
	}

	/**
	 * @param list<array<int|string, string>> $arrs
	 * @param list<array<int, string>> $arrs2
	 */
	public function arrayUnpacking(array $arrs, array $arrs2): void
	{
		assertType('array<int|string, string>', array_intersect_key(...$arrs));
		assertType('array<int, string>', array_intersect_key(...$arrs2));
		assertType('array<int, string>', array_intersect_key(...$arrs, ...$arrs2));
		assertType('array<int, string>', array_intersect_key(...$arrs2, ...$arrs));
	}

	/** @param list<string> $arr */
	public function list(array $arr, array $otherArrs): void
	{
		assertType('list<string>', array_intersect_key($arr, ['foo', 'bar']));
		/** @var array<int, string> $otherArrs */
		assertType('array<int<0, max>, string>', array_intersect_key($arr, $otherArrs));
		/** @var array<string, int> $otherArrs */
		assertType('array{}', array_intersect_key($arr, $otherArrs));
		/** @var list<string|int> $otherArrs */
		assertType('list<string>', array_intersect_key($arr, $otherArrs));
	}

}
