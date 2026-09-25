<?php

namespace ConstInFunctions;

use function PHPStan\Testing\assertType;
use const CONDITIONAL;

const TABLE_NAME = 'resized_images';
define('ANOTHER_NAME', 'foo');
define('ConstInFunctions\\ANOTHER_NAME', 'bar');

assertType('\'resized_images\'', TABLE_NAME);
assertType('\'foo\'', \ANOTHER_NAME);
assertType('\'bar\'', ANOTHER_NAME);
assertType('\'resized_images\'', \ConstInFunctions\TABLE_NAME);
assertType('\'bar\'', \ConstInFunctions\ANOTHER_NAME);

if (rand(0, 1)) {
	define('CONDITIONAL', true);
} else {
	define('CONDITIONAL', false);
}

assertType('bool', CONDITIONAL);
assertType('bool', \CONDITIONAL);

function () {
	assertType('\'resized_images\'', TABLE_NAME);
	assertType('\'foo\'', \ANOTHER_NAME);
	assertType('\'bar\'', ANOTHER_NAME);
	assertType('\'resized_images\'', \ConstInFunctions\TABLE_NAME);
	assertType('\'bar\'', \ConstInFunctions\ANOTHER_NAME);

	if (CONDITIONAL) {
	} else {
	}

	assertType('bool', CONDITIONAL);
	assertType('bool', \CONDITIONAL);
};
