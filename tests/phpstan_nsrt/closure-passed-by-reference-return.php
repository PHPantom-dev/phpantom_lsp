<?php

namespace ClosurePassedByReference;

use function PHPStan\Testing\assertType;

function () {
	$fooOrNull = null;
	assertType('null', $fooOrNull);
	$callback = function () use (&$fooOrNull): void {
		assertType('ClosurePassedByReference\Foo|null', $fooOrNull);
		if ($fooOrNull === null) {
			$fooOrNull = new Foo();
		}

		assertType('ClosurePassedByReference\Foo', $fooOrNull);

		return $fooOrNull;
	};

	assertType('ClosurePassedByReference\Foo|null', $fooOrNull);
};
