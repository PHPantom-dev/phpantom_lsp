<?php

namespace AlreadyDefinedConstant;

use function PHPStan\Testing\assertType;
use const ALREADY_DEFINED_CONSTANT;

if (rand(0, 1)) {
	define('ALREADY_DEFINED_CONSTANT', true);
} else {
	define('ALREADY_DEFINED_CONSTANT', false);
}

assertType('bool', ALREADY_DEFINED_CONSTANT);
assertType('bool', \ALREADY_DEFINED_CONSTANT);
