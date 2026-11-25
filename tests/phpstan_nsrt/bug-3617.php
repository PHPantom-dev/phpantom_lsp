<?php

namespace Bug3617;

use function PHPStan\Testing\assertType;

function (): void {
	$var = 'TEST';
	try {
		$var = 1;
		$var = test();
	} catch (\Throwable $t) {
		assertType('1', $var);
	}
};

function (): void {
	$var = 'TEST';
	try {
		$var = test();
	} catch (\Throwable $t) {
		assertType('\'TEST\'', $var);
	}
};
