<?php

namespace ArrowFunctionReturnTypeInference;

use function PHPStan\Testing\assertType;

function (int $i): void {
	$fn = fn () => $i;
	assertType('int', $fn());
};

function (int $i): void {
	$fn = fn (): string => $i;
	assertType('string', $fn());
};
