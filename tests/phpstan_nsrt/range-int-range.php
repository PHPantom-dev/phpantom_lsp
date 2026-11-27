<?php

namespace RangeIntRange;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param int<0,max> $a
	 * @param int<0,max> $b
	 */
	public function zeroToMax(
		int $a,
		int $b
	): void
	{
		assertType('non-empty-list<int<0, max>>', range($a, $b));
	}

	/**
	 * @param int<2,10> $a
	 * @param int<5,20> $b
	 */
	public function twoToTwenty(
		int $a,
		int $b
	): void
	{
		assertType('non-empty-list<int<2, 20>>', range($a, $b));
	}

	/**
	 * @param int<10,30> $a
	 * @param int<5,20> $b
	 */
	public function fifteenTo5(
		int $a,
		int $b
	): void
	{
		assertType('non-empty-list<int<5, 30>>', range($a, $b));
	}

	public function knownRange(
	): void
	{
		$a = 5;
		$b = 10;
		assertType('array{5, 6, 7, 8, 9, 10}', range($a, $b));
	}

	public function knownLargeRange(
	): void
	{
		$a = 5;
		$b = 100;
		assertType('non-empty-list<int<5, 100>>', range($a, $b));
	}
}
