<?php

namespace CastToNumericString;

use function PHPStan\Testing\assertType;

/**
 * @param int|float|numeric-string $numeric
 * @param numeric $numeric2
 * @param number $number
 * @param positive-int $positive
 * @param negative-int $negative
 * @param 1 $constantInt
 */
function foo(int $a, float $b, $numeric, $numeric2, $number, $positive, $negative, $constantInt): void {
	assertType('numeric-string', (string)$numeric);
	assertType('numeric-string', (string)$numeric2);
	assertType("'1'", (string)$constantInt);
}

/**
 * @param int|float|numeric-string $numeric
 * @param numeric $numeric2
 * @param number $number
 * @param positive-int $positive
 * @param negative-int $negative
 * @param 1 $constantInt
 */
function concatEmptyString(int $a, float $b, $numeric, $numeric2, $number, $positive, $negative, $constantInt): void {
	assertType("'1'", '' . $constantInt);

	assertType("'1'", $constantInt . '');
}

function concatAssignEmptyString(int $i, float $f) {
	$i .= '';

	$s = '';
	$s .= $f;
}

/**
 * @param int<0, max> $positive
 * @param int<min, 0> $negative
 */
function integerRangeToString($positive, $negative)
{

	if ($positive !== 0) {
	}
	if ($negative !== 0) {
	}
}
