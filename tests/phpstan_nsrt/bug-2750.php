<?php

namespace Analyser\Bug2750;

use function PHPStan\Testing\assertType;

function (array $input) {
	\assert(count($input) > 0);
	array_shift($input);
	assertType('int<0, max>', count($input));

	\assert(count($input) > 0);
	array_pop($input);
	assertType('int<0, max>', count($input));

	\assert(count($input) > 0);
	array_unshift($input, 'test');

	\assert(count($input) > 0);
	array_push($input, 'nope');
};
