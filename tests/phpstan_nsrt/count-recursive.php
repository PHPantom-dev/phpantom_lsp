<?php

namespace CountRecursive;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	/**
	 * @param array<array<mixed>> $muliDimArr
	 * @return void
	 */
	public function countMultiDim(array $muliDimArr, $mixed): void
	{
		if (count($muliDimArr, $mixed) > 2) {
		}
	}

	public function countUnknownArray(array $arr): void
	{
		assertType('array', $arr);
		assertType('int<0, max>', count($arr));
		assertType('int<0, max>', count($arr, COUNT_NORMAL));
		assertType('int<0, max>', count($arr, COUNT_RECURSIVE));
	}

	public function countEmptyArray(array $arr): void
	{
		if (count($arr) == 0) {
			assertType('array{}', $arr);
			assertType('0', count($arr));
			assertType('0', count($arr, COUNT_NORMAL));
			assertType('0', count($arr, COUNT_RECURSIVE));
		}
	}

	public function countArray(array $arr): void
	{
		if (count($arr) > 2) {
			assertType('non-empty-array', $arr);
		}
	}

	public function countArrayNormal(array $arr): void
	{
		if (count($arr, COUNT_NORMAL) > 2) {
			assertType('non-empty-array', $arr);
		}
	}

	public function countArrayRecursive(array $arr): void
	{
		if (count($arr, COUNT_RECURSIVE) > 2) {
			assertType('non-empty-array', $arr);
		}
	}

	public function countArrayUnionMode(array $arr): void
	{
		$mode = rand(0,1) ? COUNT_NORMAL : COUNT_RECURSIVE;
		if (count($arr, $mode) > 2) {
			assertType('non-empty-array', $arr);
		}
	}

	/** @param list<int> $list */
	public function countList($list): void
	{
		if (count($list) > 2) {
		}
	}

	/** @param list<int> $list */
	public function countListNormal($list): void
	{
		if (count($list, COUNT_NORMAL) > 2) {
		}
	}

	public function countImplicitNormal($mode): void
	{
		$arr = [1, 2, 3];
		if (count($arr, $mode) > 2) {
			assertType('3', count($arr));
			assertType('3', count($arr, $mode));
			assertType('3', count($arr, COUNT_NORMAL));
			assertType('3', count($arr, COUNT_RECURSIVE));
		}
	}

	public function countMixed($arr, $mode): void
	{
		if (count($arr, $mode) > 2) {
			assertType('int<0, max>', count($arr));
			assertType('int<0, max>', count($arr, COUNT_NORMAL));
			assertType('int<0, max>', count($arr, COUNT_RECURSIVE));
		}
	}

	/** @param list<int> $list */
	public function countListRecursive($list): void
	{
		if (count($list, COUNT_RECURSIVE) > 2) {
		}
	}

	/** @param arary<int> $array */
	public function countListRecursiveOnUnionOfRanges($array): void
	{
		if (!array_key_exists(5, $array)) {
			return;
		}

		if (
			(count($array) > 2 && count($array) < 5)
			|| (count($array) > 20 && count($array) < 50)
		) {
		}
	}


	public function countConstantArray(array $anotherArray): void {
		$arr = [1, 2, 3, [4, 5]];
		assertType('4', count($arr));
		assertType('4', count($arr, COUNT_NORMAL));

		$arr = [1, 2, 3, $anotherArray];
		assertType('array{1, 2, 3, array}', $arr);
		assertType('4', count($arr));
		assertType('4', count($arr, COUNT_NORMAL));

		if (rand(0,1)) {
			$arr[] = 10;
		}
		assertType('array{1, 2, 3, array}|array{1, 2, 3, array, 10}', $arr); // PHPStan spells the same type as one shape with an optional entry
		assertType('int<4, 5>', count($arr));
		assertType('int<4, 5>', count($arr, COUNT_NORMAL));

		$arr = [1, 2, 3] + $anotherArray;
	}

	public function countAfterKeyExists(array $array, int $i): void {
		if (array_key_exists(5, $array)) {
		}

		if ($array !== []) {
			assertType('non-empty-array', $array);
			if (array_key_exists(5, $array)) {

				if (array_key_exists(15, $array)) {
				}
			}
		}
	}

	public function unionIntegerCountAfterKeyExists(array $array, int $i): void {
		if ($array === []) {
			return;
		}

		assertType('non-empty-array', $array);
		if (count($array) === 3 || count($array) === 4) {
			assertType('3|4', count($array));
			if (array_key_exists(5, $array)) {
				assertType('3|4', count($array));
			}
		}
	}

	public function countMaybeCountable(array $arr, bool $b, int $i) {
		$c = rand(0,1) ? $arr : $b;
		assertType('array|bool', $c);
		assertType('int<0, max>', count($c, $i));

		if ($arr === []) {
			return;
		}

		$c = rand(0,1) ? $arr : $b;
		assertType('non-empty-array|bool', $c);
		assertType('int<0, max>', count($c, $i));

	}
}
