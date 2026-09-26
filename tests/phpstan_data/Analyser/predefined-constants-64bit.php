<?php

use function PHPStan\Testing\assertType;

// core, https://www.php.net/manual/en/reserved.constants.php
// PHPantom assumes a 64-bit build.
assertType('9223372036854775807', PHP_INT_MAX);
assertType('-9223372036854775808', PHP_INT_MIN);
