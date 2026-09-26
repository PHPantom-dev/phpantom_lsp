<?php

namespace TypeSpecifierEqual;

use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo(string $s): void
	{
		assertType("string", $s);
		if ($s == 'one') {
			assertType("'one'", $s);
		} else {
			assertType("string", $s);
		}
		assertType("string", $s);
	}

	/** @param 'one'|'two' $s */
	public function doBar(string $s): void
	{
		assertType("'one'|'two'", $s);
		if ($s == 'one') {
			assertType("'one'", $s);
		} else {
			assertType("'two'", $s);
		}
		assertType("'one'|'two'", $s);
	}

	/** @param int<1, 3>|int<8, 13> $i */
	public function doBaz(int $i): void
	{
		assertType('int<1, 3>|int<8, 13>', $i);
		if ($i == 3) {
		} else {
		}
		assertType('int<1, 3>|int<8, 13>', $i);
	}

	public function doLorem(float $f): void
	{
		assertType('float', $f);
		if ($f == 3.5) {
			assertType('3.5', $f);
		} else {
			assertType('float', $f);
		}

		assertType('float', $f);
	}

	public function doIpsum(array $a): void
	{
		assertType('array', $a);
		if ($a == []) {
			assertType('array{}', $a);
		} else {
		}
		assertType('array', $a);
	}

	public function stdClass(\stdClass $a, \stdClass $b): void
	{
		if ($a == $a) {
			assertType('stdClass', $a);
		} else {
		}

		if ($b != $b) {
		} else {
			assertType('stdClass', $b);
		}

		if ($a == $b) {
			assertType('stdClass', $a);
			assertType('stdClass', $b);
		} else {
			assertType('stdClass', $a);
			assertType('stdClass', $b);
		}

		if ($a != $b) {
			assertType('stdClass', $a);
			assertType('stdClass', $b);
		} else {
			assertType('stdClass', $a);
			assertType('stdClass', $b);
		}

		assertType('stdClass', $a);
		assertType('stdClass', $b);
	}

	/**
	 * @param array{a: string, b: array{c: string|null}} $a
	 */
	public function arrayOffset(array $a): void
	{
		if (strlen($a['a']) > 0 && $a['a'] === $a['b']['c']) {
		}
	}

}

class Bar
{

	public function doFoo(\stdClass $a, \stdClass $b): void
	{
		assertType('bool', $a == $b);
		assertType('bool', $a != $b);

		assertType('bool', self::createStdClass() == self::createStdClass());
		assertType('bool', self::createStdClass() != self::createStdClass());
	}

	public static function createStdClass(): \stdClass
	{

	}

}

class Baz
{

	public function doFoo(string $a, float $c): void
	{
		$nullableA = $a;
		if (rand(0, 1)) {
			$nullableA = null;
		}

		assertType('bool', $a == $nullableA);
		assertType('bool', $a == 'a');

		assertType('bool', $a != $nullableA);
		assertType('bool', $a != 'a');

		assertType('bool', $a == 1);

		assertType('bool', $c == 'a');
		assertType('bool', $c == 1);
		assertType('bool', $c == 1.2);
	}

}
