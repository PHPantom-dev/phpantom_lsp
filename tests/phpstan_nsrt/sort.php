<?php declare(strict_types=1);

namespace Sort;

use function PHPStan\Testing\assertNativeType;
use function PHPStan\Testing\assertType;
use function sort;
use function usort;

class Foo
{
	public function constantArray(): void
	{
		$arr = [
			4,
			'one' => 1,
			'five' => 5,
			'three' => 3,
		];

		$arr1 = $arr;
		sort($arr1);
		assertType('non-empty-list<1|3|4|5>', $arr1);
		assertNativeType('non-empty-list<1|3|4|5>', $arr1);

		$arr2 = $arr;
		rsort($arr2);
		assertType('non-empty-list<1|3|4|5>', $arr2);
		assertNativeType('non-empty-list<1|3|4|5>', $arr2);

		$arr3 = $arr;
		usort($arr3, fn(int $a, int $b) => $a <=> $b);
		assertType('non-empty-list<1|3|4|5>', $arr3);
		assertNativeType('non-empty-list<1|3|4|5>', $arr3);
	}

	public function constantArrayOptionalKey(): void
	{
		$arr = [
			'one' => 1,
			'five' => 5,
		];
		if (rand(0, 1)) {
			$arr['two'] = 2;
		}

		$arr1 = $arr;
		sort($arr1);
		assertType('non-empty-list<1|2|5>', $arr1);
		assertNativeType('non-empty-list<1|2|5>', $arr1);

		$arr2 = $arr;
		rsort($arr2);
		assertType('non-empty-list<1|2|5>', $arr2);
		assertNativeType('non-empty-list<1|2|5>', $arr2);

		$arr3 = $arr;
		usort($arr3, fn(int $a, int $b) => $a <=> $b);
		assertType('non-empty-list<1|2|5>', $arr3);
		assertNativeType('non-empty-list<1|2|5>', $arr3);
	}

	public function constantArrayUnion(): void
	{
		$arr = rand(0, 1) ? [
			'one' => 1,
			'five' => 5,
		] : [
			'two' => 2,
		];

		$arr1 = $arr;
		sort($arr1);
		assertType('non-empty-list<1|2|5>', $arr1);
		assertNativeType('non-empty-list<1|2|5>', $arr1);

		$arr2 = $arr;
		rsort($arr2);
		assertType('non-empty-list<1|2|5>', $arr2);
		assertNativeType('non-empty-list<1|2|5>', $arr2);

		$arr3 = $arr;
		usort($arr3, fn(int $a, int $b) => $a <=> $b);
		assertType('non-empty-list<1|2|5>', $arr3);
		assertNativeType('non-empty-list<1|2|5>', $arr3);
	}

	/** @param array<mixed, string> $arr */
	public function normalArray(array $arr): void
	{
		$arr1 = $arr;
		sort($arr1);
		assertType('list<string>', $arr1);
		assertNativeType('list', $arr1);

		$arr2 = $arr;
		rsort($arr2);
		assertType('list<string>', $arr2);
		assertNativeType('list', $arr2);

		$arr3 = $arr;
		usort($arr3, fn(int $a, int $b) => $a <=> $b);
		assertType('list<string>', $arr3);
		assertNativeType('list', $arr3);
	}

	public function mixed($arr): void
	{
		$arr1 = $arr;
		sort($arr1);
		assertType('list', $arr1); // PHPStan keeps `mixed`, but a sort that returns leaves a list (as it says for `shuffle()`)
		assertNativeType('mixed', $arr1);

		$arr2 = $arr;
		rsort($arr2);
		assertType('list', $arr2); // PHPStan keeps `mixed`, but a sort that returns leaves a list (as it says for `shuffle()`)
		assertNativeType('mixed', $arr2);

		$arr3 = $arr;
		usort($arr3, fn(int $a, int $b) => $a <=> $b);
		assertType('list', $arr3); // PHPStan keeps `mixed`, but a sort that returns leaves a list (as it says for `shuffle()`)
		assertNativeType('mixed', $arr3);
	}

	public function notArray(): void
	{
		$arr = 'foo';
		sort($arr);
		assertType("'foo'", $arr); // SKIP: a by-reference argument PHP rejects takes the parameter's type
	}
}

class Bar
{

	/**
	 * @template T
	 * @param T&list<array{a: scalar, b: true}> $array
	 * @return list<T>
	 */
	public function doFoo(array $array)
	{
		assertType('list<array{a: bool|float|int|string, b: true}>&T (method Sort\Bar::doFoo(), argument)', $array); // SKIP: `scalar` inside an intersection with a template is not spelled out
		usort($array, function (array $a, array $b) {
			return $a['a'] <=> $b['a'];
		});

		assertType('list<array{a: bool|float|int|string, b: true}>&T (method Sort\Bar::doFoo(), argument)', $array); // SKIP: `scalar` inside an intersection with a template is not spelled out

		return $array;
	}

}
