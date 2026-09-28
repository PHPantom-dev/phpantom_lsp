<?php

namespace GetObjectVars;

use function PHPStan\Testing\assertType;

/**
 * @param object{1: mixed} $object
 */
function getObjectVarsWithIntKeyTest(object $object): void
{
	assertType('array<mixed>', get_object_vars($object));
	assertType('array<mixed>', get_object_vars(json_decode('{"1": "test"}')));
}
