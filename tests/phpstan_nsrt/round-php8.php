<?php // lint >= 8.0

namespace RoundFamilyTestPHP8;

use function PHPStan\Testing\assertType;

$maybeNull = null;
if (rand(0, 1)) {
	$maybeNull = 1.0;
}

// Round
assertType('float', round(123));
assertType('float', round(123.456));
assertType('float', round($_GET['foo'] / 60));
assertType('float', round('123'));
assertType('float', round('123.456'));
assertType('float', round(null));
assertType('float', round($maybeNull));
assertType('float', round(true));
assertType('float', round(false));
assertType('float', round($_GET['foo']));

// Ceil
assertType('float', ceil(123));
assertType('float', ceil(123.456));
assertType('float', ceil($_GET['foo'] / 60));
assertType('float', ceil('123'));
assertType('float', ceil('123.456'));
assertType('float', ceil(null));
assertType('float', ceil($maybeNull));
assertType('float', ceil(true));
assertType('float', ceil(false));
assertType('float', ceil($_GET['foo']));

// Floor
assertType('float', floor(123));
assertType('float', floor(123.456));
assertType('float', floor($_GET['foo'] / 60));
assertType('float', floor('123'));
assertType('float', floor('123.456'));
assertType('float', floor(null));
assertType('float', floor($maybeNull));
assertType('float', floor(true));
assertType('float', floor(false));
assertType('float', floor($_GET['foo']));
