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
			assertType("'1'|'a'", $string); // SKIP: deliberately wider: a numeric string also loosely equals other spellings of its number (' 1' == '1')
		}
		if (in_array($string, [1, 'a'])) {
			assertType("string", $string); // could be '1'|'a'
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
			assertType('int|string', $stringOrInt); // could be '1'|'2'|1|2
		}
		if (in_array($stringOrNull, ['1', 'a'])) {
			assertType('string|null', $stringOrNull); // could be '1'|'a'
		}
	}
}
