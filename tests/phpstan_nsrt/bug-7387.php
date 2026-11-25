<?php

namespace Bug7387;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	/**
	 * @param int<-1, 5> $intRange
	 */
	public function inputTypes(int $i, float $f, string $s, int $intRange) {
		// https://3v4l.org/iXaDX
		assertType('numeric-string', sprintf('%.14F', $i));
		assertType('numeric-string', sprintf('%.14F', $f));
		assertType('numeric-string', sprintf('%.14F', $s));

		assertType('numeric-string', sprintf('%1.14F', $i));
		assertType('numeric-string', sprintf('%2.14F', $f));
		assertType('numeric-string', sprintf('%3.14F', $s));

		assertType('numeric-string', sprintf('%14F', $i));
		assertType('numeric-string', sprintf('%14F', $f));
		assertType('numeric-string', sprintf('%14F', $s));

	}

	public function specifiers(int $i) {
		// https://3v4l.org/fmVIg


		assertType('numeric-string', sprintf('%14E', $i));
		assertType('numeric-string', sprintf('%14F', $i));
		assertType('numeric-string', sprintf('%14G', $i));
		assertType('numeric-string', sprintf('%14H', $i));

	}

	/**
	 * @param positive-int $posInt
	 * @param negative-int $negInt
	 * @param int<1, 5>    $nonZeroIntRange
	 * @param int<-1, 5>   $intRange
	 */
	public function positionalArgs($mixed, int $i, float $f, string $s, int $posInt, int $negInt, int $nonZeroIntRange, int $intRange) {
		// https://3v4l.org/vVL0c

		// https://3v4l.org/1ECIq
		assertType('non-falsy-string', sprintf("%2$'#6s", $mixed, 1));
		assertType('non-falsy-string', sprintf("%2$'#6s", $mixed, $i));
		assertType('non-falsy-string', sprintf("%2$'#6s", $mixed, $posInt));
		assertType('non-falsy-string', sprintf("%2$'#6s", $mixed, $negInt));
		assertType('non-falsy-string', sprintf("%2$'#6s", $mixed, $nonZeroIntRange));

		assertType("'1'", sprintf('%2$s', $mixed, '1'));
		assertType("'abc'", sprintf('%2$s', $mixed, 'abc'));

		assertType('numeric-string', sprintf('%2$.14F', $mixed, $i));
		assertType('numeric-string', sprintf('%2$.14F', $mixed, $f));
		assertType('numeric-string', sprintf('%2$.14F', $mixed, $s));

		assertType('numeric-string', sprintf('%2$1.14F', $mixed, $i));
		assertType('numeric-string', sprintf('%2$2.14F', $mixed, $f));
		assertType('numeric-string', sprintf('%2$3.14F', $mixed, $s));

		assertType('numeric-string', sprintf('%2$14F', $mixed, $i));
		assertType('numeric-string', sprintf('%2$14F', $mixed, $f));
		assertType('numeric-string', sprintf('%2$14F', $mixed, $s));

	}

	public function invalidPositionalArgFormat($mixed, string $s) {
		assertType('string', sprintf('%0$14F', $mixed, $s));
	}

	public function escapedPercent(int $i) {
		// https://3v4l.org/2m50L
	}

	public function vsprintf(array $array)
	{
		assertType('numeric-string', vsprintf("%4d", $array));
		// too many arguments.. php silently allows it
	}
}
