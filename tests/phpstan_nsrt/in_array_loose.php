<?php // lint >= 8.0

namespace InArrayLoose;

use function PHPStan\Testing\assertType;

class Foo
{
	public function looseComparison(
		string $string,
		int $int,
		float $float,
		bool $bool,
		string|int $stringOrInt,
		string|null $stringOrNull,
	): void {
		if (in_array($string, ['1', 'a'])) {
			assertType("'a'|numeric-string", $string); // PHPStan says '1'|'a', which misses ' 1' == '1'
		}
		if (in_array($string, [1, 'a'])) {
			assertType("'a'|numeric-string", $string); // PHPStan says string; 'abc' == 1 is false in PHP 8
		}
		if (in_array($int, [1, 2])) {
			assertType('1|2', $int);
		}
		if (in_array($int, ['1', 2])) {
			assertType('int', $int); // could be 1|2
		}
		if (in_array($bool, [true])) {
		}
		if (in_array($bool, [true, null])) {
			assertType('bool', $bool);
		}
		if (in_array($float, [1.0, 2.0])) {
			assertType('1.0|2.0', $float);
		}
		if (in_array($float, ['1', 2.0])) {
			assertType('float', $float); // could be 1.0|2.0
		}
		if (in_array($stringOrInt, ['1', '2'])) {
			assertType('int|numeric-string', $stringOrInt); // PHPStan says int|string; could be '1'|'2'|1|2
		}
		if (in_array($stringOrNull, ['1', 'a'])) {
			assertType("'a'|numeric-string", $stringOrNull); // PHPStan says string|null, but null == '1' and null == 'a' are both false
		}
	}
}
