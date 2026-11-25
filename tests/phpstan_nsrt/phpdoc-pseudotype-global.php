<?php

use function PHPStan\Testing\assertType;

function () {
	/** @var Number $number */
	$number = doFoo();

	/** @var Boolean $boolean */
	$boolean = doFoo();

	/** @var Numeric $numeric */
	$numeric = doFoo();

	/** @var Never $never */
	$never = doFoo();

	/** @var Resource $resource */
	$resource = doFoo();

	/** @var Double $double */
	$double = doFoo();

	assertType('float|int', $number); // SKIP
	assertType('float|int|numeric-string', $numeric); // SKIP
	assertType('bool', $boolean);
	assertType('resource', $resource);
	assertType('never', $never);
	assertType('float', $double);
};
