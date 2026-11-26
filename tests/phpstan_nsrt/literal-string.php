<?php

namespace LiteralString;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param literal-string $literalString
	 * @param numeric-string $numericString
	 */
	public function doFoo($literalString, string $string, $numericString)
	{
		assertType('literal-string', $literalString);
		assertType('literal-string', $literalString . '');
		assertType('literal-string', '' . $literalString);
		assertType('literal-string&non-falsy-string', $literalString . 'foo');
		assertType('literal-string&non-falsy-string', 'foo' . $literalString);
		assertType('string', $string . '');
		assertType('string', '' . $string);
		assertType('string', $literalString . $string);
		assertType('string', $string . $literalString);
		assertType('literal-string', $literalString . $literalString);

		assertType('string', str_repeat($string, 10));
		assertType("''", str_repeat('', 10));
		assertType("'foofoofoofoofoofoofoofoofoofoo'", str_repeat('foo', 10));
		assertType(
			"'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'",
			str_repeat('a', 99)
		);
		assertType('\'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\'', str_repeat('a', 100)); // PHPStan says `literal-string&…`; PHPantom is more precise here
		assertType('\'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\'', str_repeat('A', 100)); // PHPStan says `literal-string&…`; PHPantom is more precise here
		assertType('\'0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000\'', str_repeat('0', 100)); // could be non-falsy-string // PHPStan says `literal-string&…`; PHPantom is more precise here
		assertType('\'1111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111\'', str_repeat('1', 100)); // PHPStan says `literal-string&…`; PHPantom is more precise here
		// Repeating a numeric type multiple times can lead to a non-numeric type: 3v4l.org/aRBdZ

		assertType("''", str_repeat('1.23', 0));
		assertType("''", str_repeat($string, 0));
		assertType("''", str_repeat($numericString, 0));

		// see https://3v4l.org/U4bM2
		$x = rand(1,2);

		$x = rand(0,2);

		$x = rand(-10,-1);
		assertType("'?,?,?,'", str_repeat('?,', 3));


		assertType('string', implode([$string]));
		assertType('literal-string', implode([$literalString]));
		assertType('string', implode($string, [$literalString]));
		assertType('literal-string', implode($literalString, [$literalString]));
		assertType('string', implode($literalString, [$string]));
	}

	/** @param literal-string $literalString */
	public function increment($literalString, string $string)
	{
		$literalString++;
		assertType('int|float|string', $literalString); // PHPStan says `literal-string`, but a numeric literal string increments to a number

		$string++;
		assertType('(float|int|string)', $string);
	}

}
