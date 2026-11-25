<?php

namespace Bug11724;

use function PHPStan\Testing\assertType;

function doFoo() {
	$f = 5.0;
	$a = [5 => 'hello'];
	if (array_key_exists($f, $a)) {
		assertType('5.0', $f);
		assertType("array{5: 'hello'}", $a);
	}
}
