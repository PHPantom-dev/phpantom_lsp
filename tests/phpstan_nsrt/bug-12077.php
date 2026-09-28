<?php // lint >= 8.3

namespace Bug12077;

use ReflectionMethod;
use function PHPStan\Testing\assertType;

function (): void {
	$methodInfo = ReflectionMethod::createFromMethodName("Exception::getMessage");
	assertType(ReflectionMethod::class, $methodInfo);
};
