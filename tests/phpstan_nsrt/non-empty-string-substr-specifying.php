<?php

namespace NonEmptyStringSubstrSpecifyinh;

use function PHPStan\Testing\assertType;

class Foo
{
	public function nonEmptySubstr(string $s, int $offset, int $length): void
	{
		if (substr($s, 10) === 'hallo') {
		}
		assertType('string', $s);
		if ('hallo' === substr($s, 10)) {
		}
		assertType('string', $s);

		if (substr($s, -10) === 'hallo') {
		}
		assertType('string', $s);
		if ('hallo' === substr($s, -10)) {
		}
		assertType('string', $s);

		if (substr($s, 10, 5) === 'hallo') {
		}
		assertType('string', $s);

		if (substr($s, 10, -5) === 'hallo') {
		}
		assertType('string', $s);

		if (substr($s, $offset) === 'hallo') {
		}
		assertType('string', $s);

		if (substr($s, $offset, $length) === 'hallo') {
		}
		assertType('string', $s);

		if (substr($s, $offset, $length) !== 'hallo') {
			assertType('string', $s);
		}
		assertType('string', $s);

		if (substr($s, $offset, $length) === '') {
			assertType('string', $s);
		}
		assertType('string', $s);
		if ('' === substr($s, $offset, $length)) {
			assertType('string', $s);
		}
		assertType('string', $s);

		if (substr($s, $offset, $length) == '') {
			assertType('string', $s);
		}
		assertType('string', $s);
		if ('' == substr($s, $offset, $length)) {
			assertType('string', $s);
		}
		assertType('string', $s);

		$x = (substr($s, 10) === 'hallo');
		assertType('string', $s);
		var_dump($x);

		$x = (substr($s, 10) !== 'hallo');
		assertType('string', $s);
		var_dump($x);

		$x = 'hallo';
		if (substr($x, 0, PHP_INT_MAX) !== 'foo') {
			assertType('\'hallo\'', $x);
		}
	}

	/**
	 * @param non-empty-string $nonES
	 * @param non-falsy-string $falsyString
	 */
	public function stringTypes(string $s, $nonES, $falsyString): void
	{
		if (substr($s, 10) === $nonES) {
		}

		if (substr($s, 10) === $falsyString) {
		}
	}
}
