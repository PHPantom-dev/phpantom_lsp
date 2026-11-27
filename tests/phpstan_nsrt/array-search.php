<?php declare(strict_types = 1);

namespace ArraySearch;

use function PHPStan\Testing\assertType;

class Foo
{

	public function nonEmpty(array $arr, string $string): void
	{
		/** @var non-empty-array<string> $arr */
		assertType('int|string|false', array_search('foo', $arr, true));
		assertType('int|string|false', array_search('foo', $arr));
		assertType('int|string|false', array_search($string, $arr, true));
	}

	public function normalArrays(array $arr, string $string): void
	{
		/** @var array<int, string> $arr */
		assertType('int|false', array_search('foo', $arr, true));
		assertType('int|false', array_search('foo', $arr));
		assertType('int|false', array_search($string, $arr, true));

		if (array_key_exists(17, $arr)) {
			assertType('int|false', array_search('foo', $arr, true));
			assertType('int|false', array_search('foo', $arr));
			assertType('int|false', array_search($string, $arr, true));
		}

		if (array_key_exists(17, $arr) && $arr[17] === 'foo') {
			assertType('int', array_search('foo', $arr, true)); // SKIP: an offset narrowed to the needle's value does not rule out `false`
			assertType('int', array_search('foo', $arr)); // SKIP: an offset narrowed to the needle's value does not rule out `false`
			assertType('int|false', array_search($string, $arr, true));
		}
	}

	public function constantArrays(array $arr, string $string): void
	{
		/** @var array{'a', 'b', 'c'} $arr */
		assertType('1', array_search('b', $arr, true));
		assertType('1', array_search('b', $arr));
		assertType('0|1|2|false', array_search($string, $arr, true));
		assertType('0|1|2|false', array_search($string, $arr, false));

		/** @var array{} $arr */
		assertType('false', array_search('b', $arr, true));
		assertType('false', array_search('b', $arr));
		assertType('false', array_search($string, $arr, true));
		assertType('false', array_search($string, $arr, false));

		/** @var array{1, '1', '2'} $arr */
		assertType('1', array_search('1', $arr, true));
		assertType('0|1', array_search('1', $arr));
		assertType('1|2|false', array_search($string, $arr, true));
		assertType('0|1|2|false', array_search($string, $arr, false));
	}

	public function constantArraysWithOptionalKeys(array $arr, string $string): void
	{
		/** @var array{0: 'a', 1?: 'b', 2: 'c'} $arr */
		assertType('1|false', array_search('b', $arr, true));
		assertType('1|false', array_search('b', $arr));
		assertType('0|1|2|false', array_search($string, $arr, true));

		/** @var array{0: 'a', 1?: 'b', 2: 'b'} $arr */
		assertType('1|2', array_search('b', $arr, true));
		assertType('1|2', array_search('b', $arr));
		assertType('0|1|2|false', array_search($string, $arr, true));
	}

	public function list(array $arr, string $string): void
	{
		/** @var list<string> $arr */
		assertType('int<0, max>|false', array_search('foo', $arr, true));
		assertType('int<0, max>|false', array_search('foo', $arr));
		assertType('int<0, max>|false', array_search($string, $arr, true));
	}

}
