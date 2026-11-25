<?php

namespace Bug8520;

use function PHPStan\Testing\assertType;

for ($i = 0; $i < 7; $i++) {
	$tryMax = true;
	while ($tryMax) {
		$tryMax = false;
	}
}

assertType('int<7, max>', $i);
