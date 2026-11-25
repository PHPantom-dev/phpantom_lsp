<?php

namespace Bug4287;

use function PHPStan\Testing\assertType;

/**
 * @param \ArrayObject<int, mixed> $obj
 */
function(\ArrayObject $obj): void {
	if (count($obj) === 0) {
		return;
	}

	assertType('int<1, max>', count($obj));

	$obj->offsetUnset(0);

	assertType('int<0, max>', count($obj));
};

/**
 * @param \ArrayObject<int, mixed> $obj
 */
function(\ArrayObject $obj): void {
	if (count($obj) === 0) {
		return;
	}

	assertType('int<1, max>', count($obj));

	unset($obj[0]);

};
