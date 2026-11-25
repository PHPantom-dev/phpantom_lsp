<?php declare(strict_types = 1);

namespace Bug13749;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	/**
	 * @param non-empty-string $nonES
	 */
	public function sayNonEmpty(string $s, string $nonES): void
	{
		if (strlen($nonES) === strlen($s)) {
		}
		if (strlen($nonES) >= strlen($s)) {
			assertType('string', $s); // could be non-empty-string
		}

		if (strlen($s) === strlen($nonES)) {
		}
		if (strlen($s) >= strlen($nonES)) {
		}
	}

	/**
	 * @param non-falsy-string $nonFalsy
	 */
	public function sayNonFalsy(string $s, string $nonFalsy): void
	{
		if (strlen($nonFalsy) === strlen($s)) {
		}
		if (strlen($nonFalsy) >= strlen($s)) {
			assertType('string', $s); // could be non-empty-string
		}

		if (strlen($s) === strlen($nonFalsy)) {
		}
		if (strlen($s) >= strlen($nonFalsy)) {
		}
	}

	/**
	 * @param non-empty-array $nonEmptyArr
	 */
	public function sayCount(array $arr, array $nonEmptyArr): void
	{
		if (count($arr) === count($nonEmptyArr)) {
			assertType('non-empty-array', $nonEmptyArr);
		}
		assertType('array', $arr);
		assertType('non-empty-array', $nonEmptyArr);

		if (count($nonEmptyArr) === count($arr)) {
			assertType('non-empty-array', $nonEmptyArr);
		}
		assertType('array', $arr);
		assertType('non-empty-array', $nonEmptyArr);
	}

}
