<?php

namespace Bug10327;

use function PHPStan\Testing\assertType;

/**
 * @param object{optionalKey?: int} $object
 * @param array{optionalKey?: int} $array
 */
function test(object $object, array $array): void
{
	$valueObject = $object->optionalKey ?? null;

	assertType('int|null', $valueObject);
}
