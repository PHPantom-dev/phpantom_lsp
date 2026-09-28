<?php // lint >= 8.0

namespace AnonymousFunction;

use function PHPStan\Testing\assertType;

function () {
	$integer = 1;
	function (string $str, ...$arr) use ($integer, $bar) {
		assertType('string', $str);
		assertType('array<int|string, mixed>', $arr);
		assertType('1', $integer);
		assertType('*ERROR*', $bar);
	};
};
