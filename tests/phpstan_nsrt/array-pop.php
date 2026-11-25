<?php declare(strict_types = 1);

namespace ArrayPop;

use function PHPStan\Testing\assertNativeType;
use function PHPStan\Testing\assertType;

class Foo
{

	public function nonEmpty(array $arr): void
	{
		/** @var non-empty-array<string> $arr */
		assertType('string', array_pop($arr));
		assertType('array<string>', $arr);
	}

	public function normalArrays(array $arr): void
	{
		/** @var array<int, string> $arr */
		assertType('string|null', array_pop($arr));
		assertType('array<int, string>', $arr);
	}

	public function compoundTypes(array $arr): void
	{
		/** @var string[]|int[] $arr */
		assertType('int|string|null', array_pop($arr));
		assertType('array<string>|array<int>', $arr); // PHPStan says `array<int|string>`; PHPantom is more precise here
	}

	public function constantArrays(array $arr): void
	{
		/** @var array{a: 0, b: 1, c: 2} $arr */
		assertType('2', array_pop($arr)); // PHPStan says `0|1|2`; PHPantom is more precise here
		assertType('array{a: 0, b: 1}', $arr);

		/** @var array{} $arr */
		assertType('null', array_pop($arr));
		assertType('array{}', $arr);
	}

	public function constantArraysWithOptionalKeys(array $arr): void
	{
		/** @var array{a?: 0, b: 1, c: 2} $arr */
		assertType('2', array_pop($arr)); // PHPStan says `0|1|2`; PHPantom is more precise here
		assertType('array{a?: 0, b: 1}', $arr);

		/** @var array{a: 0, b?: 1, c: 2} $arr */
		assertType('2', array_pop($arr)); // PHPStan says `0|1|2`; PHPantom is more precise here
		assertType('array{a: 0, b?: 1}', $arr);

		/** @var array{a: 0, b: 1, c?: 2} $arr */
		assertType('2|1', array_pop($arr)); // PHPStan says `0|1|2`; PHPantom is more precise here
		assertType('array{a: 0, b?: 1}', $arr); // SKIP: removing an entry from a shape with optional keys gives the generic array

		/** @var array{a?: 0, b?: 1, c?: 2} $arr */
		assertType('0|1|2|null', array_pop($arr));
		assertType('array{a?: 0, b?: 1}', $arr); // SKIP: removing an entry from a shape with optional keys gives the generic array
	}

	public function list(array $arr): void
	{
		/** @var list<string> $arr */
		assertType('string|null', array_pop($arr));
		assertType('list<string>', $arr);
	}

	public function mixed($mixed): void
	{
		assertType('mixed', array_pop($mixed));
		assertType('array', $mixed);
	}

	public function foo1($mixed): void
	{
		if(is_array($mixed)) {
			assertType('mixed', array_pop($mixed));
		} else {
			assertType('mixed', array_pop($mixed));
		}
	}

	/** @param non-empty-array<string> $arr1 */
	public function nativeTypes(array $arr1, array $arr2): void
	{
		assertType('string', array_pop($arr1));
		assertType('array<string>', $arr1);

		assertNativeType('mixed', array_pop($arr2));
		assertNativeType('array', $arr2);
	}

}
