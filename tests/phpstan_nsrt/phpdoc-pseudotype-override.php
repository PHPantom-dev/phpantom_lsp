<?php

use Foo\Number;
use Foo\Numeric;
use Foo\Boolean;
use Foo\Resource;
use Foo\Never;
use Foo\Double;

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

	assertType('Foo\Number', $number);
	assertType('Foo\Numeric', $numeric);
	assertType('Foo\Boolean', $boolean);
	assertType('Foo\Resource', $resource);
	// PHPantom is more precise than PHPStan here: `never` has been a reserved type name since PHP 8.1, so no class Foo\Never can be declared or imported under PHP 8.5. `@var Never` can only mean the never type.
	assertType('never', $never);
	assertType('Foo\Double', $double);
};
