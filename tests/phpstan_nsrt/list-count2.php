<?php

namespace ListCount2;

use function PHPStan\dumpType;
use function PHPStan\debugScope;
use function PHPStan\Testing\assertType;

class HelloWorld
{
	/**
	 * @param non-empty-list<int> $listA
	 * @param list<int> $listB
	 */
	public function sayIdenticalLists($listA, array $listB): void
	{
		if (count($listA) === count($listB)) {
			assertType('non-empty-list<int>', $listA);
		}
		assertType('non-empty-list<int>', $listA);
		assertType('list<int>', $listB);
	}

	/**
	 * @param non-empty-list<int> $listA
	 */
	public function sayIdenticalList($listA, array $arrB): void
	{
		if (count($listA) === count($arrB)) {
			assertType('non-empty-list<int>', $listA);
		}
		assertType('non-empty-list<int>', $listA);
		assertType('array', $arrB);
	}

	/**
	 * @param non-empty-array<int> $arrA
	 */
	public function sayEqualArray($arrA, array $arrB): void
	{
		if (count($arrA) == count($arrB)) {
			assertType('non-empty-array<int>', $arrA);
		}
		assertType('non-empty-array<int>', $arrA);
		assertType('array', $arrB);
	}

	/**
	 * @param non-empty-array<int> $arrA
	 * @param array<int> $arrB
	 */
	public function sayEqualIntArray($arrA, array $arrB): void
	{
		if (count($arrA) == count($arrB)) {
			assertType('non-empty-array<int>', $arrA);
		}
		assertType('non-empty-array<int>', $arrA);
		assertType('array<int>', $arrB);
	}

	/**
	 * @param non-empty-array<int> $arrA
	 * @param array<string> $arrB
	 */
	public function sayEqualStringArray($arrA, array $arrB): void
	{
		if (count($arrA) == count($arrB)) {
			assertType('non-empty-array<int>', $arrA);
		}
		assertType('non-empty-array<int>', $arrA);
		assertType('array<string>', $arrB);
	}

	/**
	 * @param array<int> $arrA
	 * @param array<string> $arrB
	 */
	public function sayUnknownSizeArray($arrA, array $arrB): void
	{
		if (count($arrA) == count($arrB)) {
			assertType('array<int>', $arrA);
			assertType('array<string>', $arrB);
		}
		assertType('array<int>', $arrA);
		assertType('array<string>', $arrB);
	}

	/**
	 * @param array{int, int, int} $arrA
	 * @param list $arrB
	 */
	function sayEqualArrayShape($arrA, array $arrB): void
	{
		if (count($arrA) == count($arrB)) {
			assertType('array{int, int, int}', $arrA);
			assertType('array{mixed, mixed, mixed}', $arrB); // SKIP: count() equality does not carry a shape's length
		}
		assertType('array{int, int, int}', $arrA);
		assertType('list', $arrB);
	}

	/**
	 * @param list $arrA
	 * @param array{int, int, int} $arrB
	 */
	function sayEqualArrayShapeReversed($arrA, array $arrB): void
	{
		if (count($arrA) == count($arrB)) {
			assertType('array{mixed, mixed, mixed}', $arrA); // SKIP: count() equality does not carry a shape's length
			assertType('array{int, int, int}', $arrB);
		}
		assertType('list', $arrA);
		assertType('array{int, int, int}', $arrB);
	}

	/**
	 * @param array{int, int, int} $arrA
	 * @param list $arrB
	 */
	function sayEqualArrayShapeAfterNarrowedCount($arrA, array $arrB): void
	{
		if (count($arrB) < 2) {
			return;
		}

		if (count($arrA) == count($arrB)) {
			assertType('array{int, int, int}', $arrA);
			assertType('array{mixed, mixed, mixed}', $arrB); // SKIP: count() equality does not carry a shape's length
		}
		assertType('array{int, int, int}', $arrA);
	}

	/**
	 * @param non-empty-array $arrB
	 */
	function dontNarrowEmpty(array $arrB): void
	{
		$arrA = [];
		assertType('array{}', $arrA);

		if (count($arrA) == count($arrB)) {
			assertType('non-empty-array', $arrB); // could be '*NEVER*'
		}
		assertType('array{}', $arrA);

		if (count($arrB) == count($arrA)) {
			assertType('non-empty-array', $arrB); // could be '*NEVER*'
		}
		assertType('array{}', $arrA);
	}

	/**
	 * @param non-empty-list<int> $listA
	 * @param list<int> $listB
	 */
	public function supportsNormalCount($listA, array $listB): void
	{
		if (count($listA, COUNT_NORMAL) === count($listB)) {
			assertType('non-empty-list<int>', $listA);
		}
		assertType('non-empty-list<int>', $listA);
		assertType('list<int>', $listB);
	}

	/**
	 * @param array{int, int, int} $arrA
	 * @param list $arrB
	 */
	function skipRecursiveLeftCount($arrA, array $arrB): void
	{
		if (count($arrB) < 2) {
			return;
		}

		if (count($arrA, COUNT_RECURSIVE) == count($arrB)) {
			assertType('array{int, int, int}', $arrA);
			assertType('array{mixed, mixed, mixed}', $arrB); // SKIP: count() equality does not carry a shape's length
		}
		assertType('array{int, int, int}', $arrA);
	}

	/**
	 * @param array{int, int, int} $arrA
	 * @param list $arrB
	 */
	function skipRecursiveRightCount($arrA, array $arrB): void
	{
		if (count($arrB) < 2) {
			return;
		}

		if (count($arrA) == count($arrB, COUNT_RECURSIVE)) {
			assertType('array{int, int, int}', $arrA);
		}
		assertType('array{int, int, int}', $arrA);
	}

	/**
	 * @param non-empty-array<int> $arrA
	 * @param array<int> $arrB
	 */
	public function skipRecursiveCount($arrA, array $arrB): void
	{
		if (count($arrA, COUNT_RECURSIVE) == count($arrB)) {
			assertType('non-empty-array<int>', $arrA);
		}
		assertType('non-empty-array<int>', $arrA);
		assertType('array<int>', $arrB);
	}

}
