<?php

namespace BitwiseNot;

use function PHPStan\Testing\assertType;

/**
 * @param string|int $stringOrInt
 * @param non-empty-string $nonEmptyString
 * @param non-falsy-string $nonFalsyString
 */
function foo(int $int, string $string, float $float, $stringOrInt, string $nonEmptyString, $nonFalsyString) : void{
	assertType('int', ~$int);
	assertType('string', ~$string);
	assertType('int', ~$float);
	assertType('int|string', ~$stringOrInt);
	assertType("'" . (~"abc") . "'", ~"abc");
	assertType('-2', ~1);
}
