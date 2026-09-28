<?php

namespace Bug4907;

use function PHPStan\Testing\assertType;

function sayHello(): void
{
	$foo = [5,6,7];
	foreach ($foo as $i => $foo) {
		// ...
	}

	assertType('5|6|7', $foo);
}
