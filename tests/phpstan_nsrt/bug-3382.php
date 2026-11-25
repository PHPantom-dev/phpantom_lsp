<?php

namespace Bug3382;

use function PHPStan\Testing\assertType;

if (ini_get('auto_prepend_file')) {
	assertType('non-falsy-string', ini_get('auto_prepend_file'));
}
