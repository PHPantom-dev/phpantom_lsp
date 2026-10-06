<?php

namespace PhpdocPseudoTypesNamespace;

use function PHPStan\Testing\assertType;

class Number {}
class Numeric {}
class Boolean {}
class Resource {}
class Double {}

function () {
	/** @var Number $number */
	$number = doFoo();

	/** @var Boolean $boolean */
	$boolean = doFoo();

	/** @var Numeric $numeric */
	$numeric = doFoo();

	/** @var Resource $resource */
	$resource = doFoo();

	/** @var Double $double */
	$double = doFoo();

	assertType('PhpdocPseudoTypesNamespace\Number', $number);
	assertType('PhpdocPseudoTypesNamespace\Numeric', $numeric);
	assertType('PhpdocPseudoTypesNamespace\Boolean', $boolean);
	assertType('PhpdocPseudoTypesNamespace\Resource', $resource);
	assertType('PhpdocPseudoTypesNamespace\Double', $double);
};
