<?php

namespace PhpDocInClosureBind;

use Closure;
use PHPStan\Analyser\Analyser;
use function PHPStan\Testing\assertType;

function ($mixed): void {
	$foo = new Analyser();
	Closure::bind(function () use ($mixed) {
		/** @var \stdClass $mixed */
		assertType('stdClass', $mixed);
	}, $foo, Analyser::class);
};
