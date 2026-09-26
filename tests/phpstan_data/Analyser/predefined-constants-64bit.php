<?php

use function PHPStan\Testing\assertType;

// core, https://www.php.net/manual/en/reserved.constants.php
// PHPantom assumes a 64-bit build.
assertType('9223372036854775807', PHP_INT_MAX);
assertType('-9223372036854775808|-2147483648', PHP_INT_MIN); // SKIP: literal operands are not folded through this operator
