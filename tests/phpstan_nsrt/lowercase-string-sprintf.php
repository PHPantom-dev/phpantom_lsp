<?php

namespace LowercaseStringSprintf;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param lowercase-string $lowercase
	 * @param lowercase-string&non-empty-string $nonEmptyLowercase
	 * @param lowercase-string&non-falsy-string $nonFalsyLowercase
	 */
	public function doSprintf(
		string $string,
		string $lowercase,
		string $nonEmptyLowercase,
		string $nonFalsyLowercase,
		bool $bool
	): void {
		$format = $bool ? 'Foo 1 %s' : 'Foo 2 %s';
		$formatLower = $bool ? 'foo 1 %s' : 'foo 2 %s';
		$constant = $bool ? 'A' : 'B';
		$constantLower = $bool ? 'a' : 'b';

		assertType("'A'|'B'", sprintf('%s', $constant));
		assertType('string', sprintf($lowercase, $constant));
		assertType('string', sprintf($string, $constant));

		assertType("'a'|'b'", sprintf('%s', $constantLower));
		assertType('string', sprintf($string, $constantLower));

		assertType('lowercase-string', sprintf('%s', $lowercase));
		assertType('non-falsy-string', sprintf($format, $lowercase));
		assertType('string', sprintf($string, $lowercase));

		assertType('lowercase-string&non-empty-string', sprintf('%s', $nonEmptyLowercase));
		assertType('non-falsy-string', sprintf($format, $nonEmptyLowercase));
		assertType('string', sprintf($string, $nonEmptyLowercase));

		assertType('lowercase-string&non-falsy-string', sprintf('%s', $nonFalsyLowercase));
		assertType('non-falsy-string', sprintf($format, $nonFalsyLowercase));
		assertType('string', sprintf($string, $nonFalsyLowercase));
	}

	/**
	 * @param lowercase-string $lowercase
	 * @param lowercase-string&non-empty-string $nonEmptyLowercase
	 * @param lowercase-string&non-falsy-string $nonFalsyLowercase
	 */
	public function doVSprintf(
		string $string,
		string $lowercase,
		string $nonEmptyLowercase,
		string $nonFalsyLowercase,
		bool $bool
	): void {
		$format = $bool ? 'Foo 1 %s' : 'Foo 2 %s';
		$formatLower = $bool ? 'foo 1 %s' : 'foo 2 %s';
		$constant = $bool ? 'A' : 'B';
		$constantLower = $bool ? 'a' : 'b';

		assertType('numeric-string', vsprintf('%d', [$constant]));
		assertType('non-falsy-string', vsprintf($format, [$constant]));
		assertType('non-falsy-string', vsprintf($formatLower, [$constant]));
		assertType('string', vsprintf($lowercase, [$constant]));
		assertType('string', vsprintf($string, [$constant]));

		assertType('non-falsy-string', vsprintf($format, [$constantLower]));
		assertType('string', vsprintf($string, [$constantLower]));

		assertType('non-falsy-string', vsprintf($format, [$lowercase]));
		assertType('string', vsprintf($string, [$lowercase]));

		assertType('non-falsy-string', vsprintf($format, [$nonEmptyLowercase]));
		assertType('string', vsprintf($string, [$nonEmptyLowercase]));

		assertType('non-falsy-string', vsprintf($format, [$nonFalsyLowercase]));
		assertType('string', vsprintf($string, [$nonFalsyLowercase]));
	}

}
