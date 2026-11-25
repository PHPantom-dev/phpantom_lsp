<?php

namespace ConstantPhpdocType;

use ConstantPhpdocType\Sub;
use ConstantPhpdocType\Sub as Aliased;
use const PREG_SPLIT_NO_EMPTY as PREG_SPLIT_NO_EMPTY_ALIAS;
use function PHPStan\Testing\assertType;

const MY_CONST = 100;
const ANOTHER_CONST = 200;
const PREG_SPLIT_NO_EMPTY_COPY = PREG_SPLIT_NO_EMPTY;

const BAR = 10;
class BAR {}

/**
 * @param MY_CONST $one
 * @param MY_CONST|ANOTHER_CONST $two
 * @param PREG_SPLIT_NO_EMPTY $three
 * @param PREG_SPLIT_NO_EMPTY|PREG_SPLIT_DELIM_CAPTURE $four
 * @param PREG_SPLIT_NO_EMPTY_COPY $five
 * @param PREG_SPLIT_NO_EMPTY_ALIAS $six
 * @param BAR $seven
 * @param Sub\Nested\CONST_FROM_OTHER_NS $eight
 * @param Aliased\Nested\CONST_FROM_OTHER_NS $nine
 * @param PHP_INT_MAX $ten
 */
function foo(
	$one,
	$two,
	$three,
	$four,
	$five,
	$six,
	$seven,
	$eight,
	$nine,
	$ten
) {
	assertType('100', $one);
	assertType('100|200', $two);
	assertType('1', $three);
	assertType('1|2', $four);
	assertType('1', $five);
	assertType('1', $six);
	assertType('ConstantPhpdocType\BAR', $seven); // classes take precedence over constants
	assertType("'foo'", $eight);
	assertType("'foo'", $nine);
	// Upstream allows for a 32-bit build; PHPantom reads `PHP_INT_MAX` as
	// the 64-bit value here, as it does where the constant is used in code.
	assertType('9223372036854775807', $ten);
}

namespace ConstantPhpdocType\Sub\Nested;

const CONST_FROM_OTHER_NS = 'foo';
