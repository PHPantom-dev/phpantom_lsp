<?php

namespace Bug2760;

use function PHPStan\Testing\assertType;

function (): void {
	$boolean = false;
	assertType('false', $boolean);

	$fn = function () use (&$boolean) : void {
		$iter = [0];

		foreach ($iter as $_) {
			$boolean = true;

			return;
		}
	};

	$fn();

	assertType('bool', $boolean);
};
