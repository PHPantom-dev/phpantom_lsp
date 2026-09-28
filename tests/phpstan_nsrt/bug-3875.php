<?php

namespace Bug3875;

use function PHPStan\Testing\assertType;

function foo(): void
{
	$queue = ['foo'];
	$list = [];
	do {
		$current = array_pop($queue);
		assertType('\'foo\'', $current);
		if ($current === null) {
			break;
		}
		$list[] = $current;
	} while ($queue);
}
