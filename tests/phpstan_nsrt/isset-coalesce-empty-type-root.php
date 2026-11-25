<?php

namespace IssetCoalesceEmptyTypeRootScope;

use function PHPStan\Testing\assertType;

$alwaysDefinedNotNullable = 'string';
if (rand(0, 1)) {
	$sometimesDefinedVariable = 1;
}

assertType('bool', isset($sometimesDefinedVariable));
assertType('bool', isset($neverDefinedVariable));


$bar = 'abc';
assertType('\'abc\'', $bar ?? false); // SKIP: `??` keeps its right side when the left side cannot be null
