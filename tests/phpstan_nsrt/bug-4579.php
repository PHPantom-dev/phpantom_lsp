<?php

namespace Bug4579;

use function PHPStan\Testing\assertType;

function (string $class): void {
	$foo = new $class();
	assertType('object', $foo);
	if (method_exists($foo, 'doFoo')) {
		assertType('object&hasMethod(doFoo)', $foo);
	}
};

function (): void {
	$s = \stdClass::class;
	if (rand(0, 1)) {
		$s = \Exception::class;
	}

	assertType('Exception|stdClass', new $s());
};
