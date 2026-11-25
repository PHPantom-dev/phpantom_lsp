<?php declare(strict_types = 1);

namespace Bug13747;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	/** @param list<int> $list */
	public function count($list): void
	{
		if (count($list) === 0) {
			return;
		}

		if (count($list) > 2) {
			assertType('bool', array_key_exists(3, $list));

			assertType('int<3, max>', count($list));
		} else {
			assertType('non-empty-list<int>', $list);
		}
		assertType('non-empty-list<int>', $list);

		if (count($list, COUNT_NORMAL) > 2) {
		} else {
			assertType('non-empty-list<int>', $list);
		}

		assertType('non-empty-list<int>', $list);
		if (count($list, COUNT_RECURSIVE) > 2) { // COUNT_RECURSIVE on non-recursive array
		} else {
			assertType('non-empty-list<int>', $list);
		}
	}

	/** @param list<int> $list */
	public function doFoo($list): void
	{
		if (count($list) === 0) {
			return;
		}

		if (count($list) >= 2) {
			assertType('int<2, max>', count($list));
		} else {
			assertType('non-empty-list<int>', $list);
		}

		if (count($list) < 5) {
			assertType('non-empty-list<int>', $list);
			assertType('int<1, 4>', count($list));
		} else {
		}
	}

	/** @param list<int> $list */
	public function doBar($list): void
	{
		if (count($list) === 0) {
			return;
		}

		if (2 <= count($list)) {
			assertType('int<2, max>', count($list));
		} else {
			assertType('non-empty-list<int>', $list);
			assertType('1', count($list));
		}
	}

	/** @param non-empty-array<int> $nonEmptyArray */
	public function doNonEmptyArray($nonEmptyArray): void
	{
		if (2 <= count($nonEmptyArray)) {
			assertType('non-empty-array<int>', $nonEmptyArray);
		} else {
			assertType('non-empty-array<int>', $nonEmptyArray);
		}

		if (count($nonEmptyArray) < 5) {
			assertType('non-empty-array<int>', $nonEmptyArray);
		} else {
			assertType('non-empty-array<int>', $nonEmptyArray);
		}
	}

	/**
	 * @param list<int> $listA
	 * @param non-empty-list<string> $listB
	 */
	public function doMaybeBar($listA, $listB): void
	{
		if (rand(0,1)) {
			$list = $listA;
		} else {
			$list = $listB;
		}

		if (2 <= count($list)) {
		} else {
			assertType('list<int>|non-empty-list<string>', $list); // PHPStan says `list<int|string>`; PHPantom is more precise here
		}
	}

	/**
	 * @param array<int> $aArray
	 * @param non-empty-list<string> $aList
	 */
	public function doMaybeArray($aArray, $aList): void
	{
		if (rand(0,1)) {
			$listOrArray = $aArray;
		} else {
			$listOrArray = $aList;
		}

		if (2 <= count($listOrArray)) {
			assertType('non-empty-array<int>|non-empty-list<string>', $listOrArray); // PHPStan says `non-empty-array<int|string>`; PHPantom is more precise here
		} else {
			assertType('array<int>|non-empty-list<string>', $listOrArray); // PHPStan says `array<int|string>`; PHPantom is more precise here
		}
	}

	/** @param list<int> $list */
	public function doMaybeEmpty($list): void
	{
		if (rand(0,1)) {
			$list = [1, 2, 3];
		}

		if (2 <= count($list)) {
		} else {
			assertType('list<int>', $list);
		}
	}

	/** @param list<int> $list */
	public function checkLimit($list): void
	{
		if (count($list) === 0) {
			return;
		}

		if (count($list) > 9) {
			assertType('int<10, max>', count($list));
		} else {
			assertType('non-empty-list<int>', $list);
		}

		if (count($list) > 10) {
			assertType('int<11, max>', count($list));
		} else {
			assertType('non-empty-list<int>', $list);
		}

	}

	/** @param array<int> $array */
	public function countArray($array): void
	{
		if (count($array) < 5) {
			assertType('array<int>', $array);
		} else {
		}

		if (count($array) === 0) {
			return;
		}

		if (count($array) > 2) {
			assertType('bool', array_key_exists(-1, $array));
			assertType('bool', array_key_exists(0, $array));
			assertType('bool', array_key_exists(1, $array));
			assertType('bool', array_key_exists(2, $array));
			assertType('bool', array_key_exists(3, $array));

			assertType('non-empty-array<int>', $array);
			assertType('int<3, max>', count($array));
		} else {
			assertType('non-empty-array<int>', $array);
		}
		assertType('non-empty-array<int>', $array);

		if (count($array, COUNT_NORMAL) > 2) {
			assertType('non-empty-array<int>', $array);
		} else {
			assertType('non-empty-array<int>', $array);
		}

		assertType('non-empty-array<int>', $array);
		if (count($array, COUNT_RECURSIVE) > 2) { // COUNT_RECURSIVE on non-recursive array
			assertType('non-empty-array<int>', $array);
		} else {
			assertType('non-empty-array<int>', $array);
		}
	}
}
