<?php

namespace Bitwise;

use function PHPStan\Testing\assertType;

// Upstream expects `*ERROR*` wherever an `int` meets a `string`. PHP
// accepts a numeric string there, so PHPantom records what the operation
// produces: `int` when one side is an int, and `int|string` when both
// sides may be strings.

/**
 * @param string|int $stringOrInt
 * @param mixed $mixed
 */
function test(int $int, string $string, $stringOrInt, $mixed) : void
{
	assertType('int', $int & $int);
	assertType('int', $int & $string);
	assertType('int', $int & $stringOrInt);
	assertType('int', $int & $mixed);
	assertType('int', $string & $int);
	assertType('string', $string & $string);
	assertType('int|string', $string & $stringOrInt);
	assertType('string', $string & $mixed);
	assertType('int', $stringOrInt & $int);
	assertType('int|string', $stringOrInt & $string);
	assertType('int|string', $stringOrInt & $stringOrInt);
	assertType('int|string', $stringOrInt & $mixed);
	assertType('int', $mixed & $int);
	assertType('string', $mixed & $string);
	assertType('int|string', $mixed & $stringOrInt);
	assertType('(int|string)', $mixed & $mixed);

	assertType('int', $int | $int);
	assertType('int', $int | $string);
	assertType('int', $int | $stringOrInt);
	assertType('int', $int | $mixed);
	assertType('int', $string | $int);
	assertType('string', $string | $string);
	assertType('int|string', $string | $stringOrInt);
	assertType('string', $string | $mixed);
	assertType('int', $stringOrInt | $int);
	assertType('int|string', $stringOrInt | $string);
	assertType('int|string', $stringOrInt | $stringOrInt);
	assertType('int|string', $stringOrInt | $mixed);
	assertType('int', $mixed | $int);
	assertType('string', $mixed | $string);
	assertType('int|string', $mixed | $stringOrInt);
	assertType('(int|string)', $mixed | $mixed);

	assertType('int', $int ^ $int);
	assertType('int', $int ^ $string);
	assertType('int', $int ^ $stringOrInt);
	assertType('int', $int ^ $mixed);
	assertType('int', $string ^ $int);
	assertType('string', $string ^ $string);
	assertType('int|string', $string ^ $stringOrInt);
	assertType('string', $string ^ $mixed);
	assertType('int', $stringOrInt ^ $int);
	assertType('int|string', $stringOrInt ^ $string);
	assertType('int|string', $stringOrInt ^ $stringOrInt);
	assertType('int|string', $stringOrInt ^ $mixed);
	assertType('int', $mixed ^ $int);
	assertType('string', $mixed ^ $string);
	assertType('int|string', $mixed ^ $stringOrInt);
	assertType('(int|string)', $mixed ^ $mixed);
}
