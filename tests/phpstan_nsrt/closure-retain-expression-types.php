<?php

namespace ClosureRetainExpressionTypes;

use function class_exists;
use function defined;
use function function_exists;
use function PHPStan\Testing\assertType;

function () {
	assertType('bool', function_exists('foo123'));
	if (function_exists('foo123')) {
		assertType('true', function_exists('foo123'));
		function () {
		};
	} else {
		assertType('false', function_exists('foo123'));
		function () {
			assertType('bool', function_exists('foo123'));
		};
	}
};
