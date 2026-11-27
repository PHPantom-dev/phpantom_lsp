<?php

namespace Bug8520;

use function PHPStan\Testing\assertType;

for ($i = 0; $i < 7; $i++) {
	$tryMax = true;
	while ($tryMax) {
		$tryMax = false;
	}
}

// PHPantom is more precise: the counter steps by one from 0, so it leaves at 7.
assertType('7', $i);
